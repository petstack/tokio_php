//! Core error types.

use std::fmt;

/// Core errors for request/response handling.
#[derive(Debug)]
pub enum Error {
    /// Invalid HTTP request.
    InvalidRequest(String),

    /// Invalid HTTP response.
    InvalidResponse(String),

    /// Request timeout.
    Timeout {
        duration_ms: u64,
    },

    /// Script execution error.
    Execution(String),

    /// I/O error.
    Io(std::io::Error),

    /// HTTP error.
    Http(http::Error),

    /// Custom error with message.
    Custom(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidRequest(msg) => write!(f, "invalid request: {}", msg),
            Error::InvalidResponse(msg) => write!(f, "invalid response: {}", msg),
            Error::Timeout { duration_ms } => write!(f, "request timeout after {}ms", duration_ms),
            Error::Execution(msg) => write!(f, "execution error: {}", msg),
            Error::Io(e) => write!(f, "I/O error: {}", e),
            Error::Http(e) => write!(f, "HTTP error: {}", e),
            Error::Custom(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Http(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<http::Error> for Error {
    fn from(e: http::Error) -> Self {
        Error::Http(e)
    }
}

impl From<String> for Error {
    fn from(msg: String) -> Self {
        Error::Custom(msg)
    }
}

impl From<&str> for Error {
    fn from(msg: &str) -> Self {
        Error::Custom(msg.to_string())
    }
}

/// Result type alias for core operations.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::InvalidRequest("missing body".to_string());
        assert_eq!(err.to_string(), "invalid request: missing body");

        let err = Error::Timeout { duration_ms: 5000 };
        assert_eq!(err.to_string(), "request timeout after 5000ms");

        let err = Error::Custom("something went wrong".to_string());
        assert_eq!(err.to_string(), "something went wrong");
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();

        assert!(matches!(err, Error::Io(_)));
        assert!(err.to_string().contains("I/O error"));
    }

    #[test]
    fn test_error_from_string() {
        let err: Error = "custom error".into();
        assert!(matches!(err, Error::Custom(_)));
        assert_eq!(err.to_string(), "custom error");

        let err: Error = String::from("another error").into();
        assert_eq!(err.to_string(), "another error");
    }
}
