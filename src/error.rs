use std::{fmt, io};

/// Errors preserve server codes so applications can distinguish authorization and SQL failures.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Io(io::Error),
    Json(serde_json::Error),
    Server { code: String, message: String },
    Protocol(&'static str),
    Timeout,
    Closed,
    ResponseTooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "ChuSQL I/O: {error}"),
            Self::Json(error) => write!(f, "ChuSQL JSON: {error}"),
            Self::Server { code, message } => write!(f, "ChuSQL {code}: {message}"),
            Self::Protocol(message) => write!(f, "ChuSQL protocol: {message}"),
            Self::Timeout => f.write_str("ChuSQL request timed out"),
            Self::Closed => f.write_str("ChuSQL connection is closed; reconnect before retrying"),
            Self::ResponseTooLarge => f.write_str("ChuSQL response exceeds configured byte limit"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
