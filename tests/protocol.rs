use chusql_driver::{Client, Error, Options};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

async fn server(script: Vec<(Value, &'static str)>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        for (expected, response) in script {
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            assert_eq!(serde_json::from_str::<Value>(&line).unwrap(), expected);
            // Split writes also exercise TCP framing across packet boundaries.
            for chunk in response.as_bytes().chunks(7) {
                stream.get_mut().write_all(chunk).await.unwrap();
            }
        }
    });
    (address, task)
}

fn handshake() -> Vec<(Value, &'static str)> {
    vec![
        (
            json!({"method":"hello","protocol":1}),
            "{\"status\":\"hello\",\"protocol\":1,\"server\":\"chusql-server\"}\n",
        ),
        (
            json!({"method":"login","user":"app","password":"秘密\npass"}),
            "{\"status\":\"ok\",\"user\":\"app\",\"admin\":false}\n",
        ),
    ]
}

#[tokio::test]
async fn authenticated_session_preserves_results_and_recovers_from_sql_errors() {
    let mut script = handshake();
    script.extend([
        (json!({"method":"query","sql":"SELECT * FROM users"}), "{\"status\":\"result\",\"columns\":[\"id\",\"name\",\"active\",\"extra\"],\"rows\":[[9223372036854775808,\"小楚\",true,null]],\"rowCount\":1,\"truncated\":true,\"database\":\"app\"}\n"),
        (json!({"method":"query","sql":"bad SQL"}), "{\"status\":\"error\",\"code\":\"syntax_error\",\"message\":\"invalid SQL\"}\n"),
        (json!({"method":"ping"}), "{\"status\":\"pong\"}\n"),
    ]);
    let (address, task) = server(script).await;
    let client = Client::connect(&address, "app", "秘密\npass")
        .await
        .unwrap();
    let result = client.query("SELECT * FROM users").await.unwrap();
    assert_eq!(
        result.rows[0],
        vec![
            json!(9223372036854775808_u64),
            json!("小楚"),
            json!(true),
            Value::Null
        ]
    );
    assert!(result.truncated);
    assert_eq!(result.row_count, 1);
    assert_eq!(result.database.as_deref(), Some("app"));
    assert!(
        matches!(client.query("bad SQL").await, Err(Error::Server { code, .. }) if code == "syntax_error")
    );
    client.clone().ping().await.unwrap();
    client.close().await;
    assert!(matches!(client.ping().await, Err(Error::Closed)));
    task.await.unwrap();
}

#[tokio::test]
async fn rejects_wrong_protocol_and_failed_login() {
    let (address, task) = server(vec![(
        json!({"method":"hello","protocol":1}),
        "{\"status\":\"hello\",\"protocol\":2}\n",
    )])
    .await;
    assert!(matches!(
        Client::connect(address, "app", "pw").await,
        Err(Error::Protocol(_))
    ));
    task.await.unwrap();
    let mut script = handshake();
    script[1].1 =
        "{\"status\":\"error\",\"code\":\"unauthorized\",\"message\":\"invalid credentials\"}\n";
    let (address, task) = server(script).await;
    assert!(
        matches!(Client::connect(address, "app", "秘密\npass").await, Err(Error::Server { code, .. }) if code == "unauthorized")
    );
    task.await.unwrap();
}

#[tokio::test]
async fn malformed_oversized_and_unterminated_responses_discard_connection() {
    for response in [
        "not JSON\n",
        "{\"status\":\"pong\"}",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n",
    ] {
        let mut script = handshake();
        script.push((json!({"method":"ping"}), response));
        let (address, task) = server(script).await;
        let client = Client::connect_with_options(
            address,
            "app",
            "秘密\npass",
            Options {
                max_response_bytes: 128,
                ..Options::default()
            },
        )
        .await
        .unwrap();
        assert!(client.ping().await.is_err());
        assert!(matches!(client.ping().await, Err(Error::Closed)));
        task.await.unwrap();
    }
}

#[tokio::test]
async fn timeout_and_cancellation_do_not_reuse_pending_responses() {
    for cancel in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (sent, received) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            for (_, response) in handshake() {
                let mut line = String::new();
                stream.read_line(&mut line).await.unwrap();
                stream
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .unwrap();
            }
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            sent.send(()).unwrap();
            line.clear();
            assert_eq!(stream.read_line(&mut line).await.unwrap(), 0);
        });
        let client = Client::connect_with_options(
            address,
            "app",
            "秘密\npass",
            Options {
                timeout: Duration::from_millis(500),
                ..Options::default()
            },
        )
        .await
        .unwrap();
        let pending = tokio::spawn({
            let client = client.clone();
            async move { client.ping().await }
        });
        received.await.unwrap();
        if cancel {
            pending.abort();
            assert!(pending.await.unwrap_err().is_cancelled());
        } else {
            assert!(matches!(pending.await.unwrap(), Err(Error::Timeout)));
        }
        assert!(matches!(client.ping().await, Err(Error::Closed)));
        task.await.unwrap();
    }
}
