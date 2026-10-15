//! An asynchronous client for ChuSQL's version 1 newline-delimited JSON protocol.
//! Clones share one authenticated session and serialize requests.
mod error;
mod query;

pub use error::Error;
pub use query::{Param, Query};
pub use serde_json::Value;

use serde::Deserialize;
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpStream, ToSocketAddrs},
    sync::Mutex,
    time::timeout,
};

/// Limits for connecting and exchanging each request (excluding time queued behind other requests).
#[derive(Debug, Clone)]
pub struct Options {
    pub timeout: Duration,
    pub max_response_bytes: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            max_response_bytes: 16 * 1024 * 1024,
        }
    }
}

/// The original wire values are preserved, including NULL and runtime type objects.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub row_count: u64,
    pub truncated: bool,
    pub database: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "status")]
enum Response {
    #[serde(rename = "hello")]
    Hello { protocol: u32 },
    #[serde(rename = "ok")]
    Login {},
    #[serde(rename = "result")]
    Result(QueryResult),
    #[serde(rename = "pong")]
    Pong,
    #[serde(rename = "error")]
    Error { code: String, message: String },
}

/// Cloneable backend application state. Each clone uses the same connection and login.
#[derive(Clone)]
pub struct Client {
    connection: Arc<Mutex<Option<BufReader<TcpStream>>>>,
    options: Options,
}

impl Client {
    /// Connect, negotiate protocol 1 and authenticate using the default limits.
    pub async fn connect(
        address: impl ToSocketAddrs,
        user: &str,
        password: &str,
    ) -> Result<Self, Error> {
        Self::connect_with_options(address, user, password, Options::default()).await
    }

    /// Connect with explicit limits. Passwords are sent over plain TCP by the server protocol.
    pub async fn connect_with_options(
        address: impl ToSocketAddrs,
        user: &str,
        password: &str,
        options: Options,
    ) -> Result<Self, Error> {
        let stream = timeout(options.timeout, TcpStream::connect(address))
            .await
            .map_err(|_| Error::Timeout)??;
        stream.set_nodelay(true)?;
        let client = Self {
            connection: Arc::new(Mutex::new(Some(BufReader::new(stream)))),
            options,
        };
        match client
            .request(json!({"method": "hello", "protocol": 1}))
            .await?
        {
            Response::Hello { protocol: 1 } => {}
            _ => return Err(Error::Protocol("expected hello with protocol 1")),
        }
        match client
            .request(json!({"method": "login", "user": user, "password": password}))
            .await?
        {
            Response::Login {} => {}
            _ => return Err(Error::Protocol("expected login acknowledgement")),
        }
        Ok(client)
    }

    /// Execute one SQL statement. The server does not support bound parameters.
    /// Never interpolate untrusted input into SQL.
    pub async fn query(&self, sql: &str) -> Result<QueryResult, Error> {
        match self.request(json!({"method": "query", "sql": sql})).await? {
            Response::Result(result) => {
                if result
                    .rows
                    .iter()
                    .any(|row| row.len() != result.columns.len())
                {
                    self.close().await;
                    return Err(Error::Protocol("row width differs from columns"));
                }
                Ok(result)
            }
            _ => {
                self.close().await;
                Err(Error::Protocol("expected query result"))
            }
        }
    }

    /// Check whether the server is reachable on this connection.
    pub async fn ping(&self) -> Result<(), Error> {
        match self.request(json!({"method": "ping"})).await? {
            Response::Pong => Ok(()),
            _ => {
                self.close().await;
                Err(Error::Protocol("expected pong"))
            }
        }
    }

    /// Close the shared connection for all clones; repeated calls are harmless.
    pub async fn close(&self) {
        self.connection.lock().await.take();
    }

    async fn request(&self, request: Value) -> Result<Response, Error> {
        let mut bytes = serde_json::to_vec(&request)?;
        bytes.push(b'\n');
        let mut guard = self.connection.lock().await;
        // Taking ownership makes cancellation discard the stream instead of reusing a stale response.
        let mut connection = guard.take().ok_or(Error::Closed)?;
        let response = timeout(self.options.timeout, async {
            connection.get_mut().write_all(&bytes).await?;
            let mut line = Vec::new();
            loop {
                let chunk = connection.fill_buf().await?;
                if chunk.is_empty() {
                    return Err(Error::Closed);
                }
                let end = chunk.iter().position(|byte| *byte == b'\n');
                let used = end.map_or(chunk.len(), |index| index + 1);
                if used > self.options.max_response_bytes.saturating_sub(line.len()) {
                    return Err(Error::ResponseTooLarge);
                }
                line.extend_from_slice(&chunk[..used]);
                connection.consume(used);
                if end.is_some() {
                    break;
                }
            }
            Ok::<Response, Error>(serde_json::from_slice(&line)?)
        })
        .await
        .map_err(|_| Error::Timeout)??;
        *guard = Some(connection);
        match response {
            Response::Error { code, message } => Err(Error::Server { code, message }),
            other => Ok(other),
        }
    }
}
