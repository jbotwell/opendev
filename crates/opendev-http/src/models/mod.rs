//! Shared types for HTTP operations.

mod retry;

pub use retry::{RetryConfig, classify_retryable_error, parse_retry_after};

/// Errors that can occur during HTTP operations.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Interrupted by user")]
    Interrupted,

    #[error("All retries exhausted: {message}")]
    RetriesExhausted { message: String },

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

/// Result of an HTTP request attempt.
#[derive(Debug)]
pub struct HttpResult {
    /// Whether the request completed successfully.
    pub success: bool,
    /// HTTP status code, if a response was received.
    pub status: Option<u16>,
    /// Response body, if available.
    pub body: Option<serde_json::Value>,
    /// Error message, if the request failed.
    pub error: Option<String>,
    /// Whether the request was interrupted by the user.
    pub interrupted: bool,
    /// Whether the failure is transient and worth retrying.
    pub retryable: bool,
    /// Unique request identifier for end-to-end tracing.
    /// Propagated via the `X-Request-ID` header.
    pub request_id: Option<String>,
    /// Value of the `Retry-After` response header, if present.
    /// Used to honor server-requested retry delays on 429/503 responses.
    pub retry_after: Option<String>,
    /// Value of the `retry-after-ms` response header, if present.
    /// More precise than `Retry-After` (milliseconds instead of seconds).
    pub retry_after_ms: Option<String>,
}

impl HttpResult {
    /// Create a successful result.
    pub fn ok(status: u16, body: serde_json::Value) -> Self {
        Self {
            success: true,
            status: Some(status),
            body: Some(body),
            error: None,
            interrupted: false,
            retryable: false,
            request_id: None,
            retry_after: None,
            retry_after_ms: None,
        }
    }

    /// Create a failed result.
    pub fn fail(error: impl Into<String>, retryable: bool) -> Self {
        Self {
            success: false,
            status: None,
            body: None,
            error: Some(error.into()),
            interrupted: false,
            retryable,
            request_id: None,
            retry_after: None,
            retry_after_ms: None,
        }
    }

    /// Create an interrupted result.
    pub fn interrupted() -> Self {
        Self {
            success: false,
            status: None,
            body: None,
            error: Some("Interrupted by user".into()),
            interrupted: true,
            retryable: false,
            request_id: None,
            retry_after: None,
            retry_after_ms: None,
        }
    }

    /// Create a result from an HTTP response with a retryable status.
    pub fn retryable_status(
        status: u16,
        body: Option<serde_json::Value>,
        retry_after: Option<String>,
    ) -> Self {
        Self {
            success: false,
            status: Some(status),
            body,
            error: Some(format!("HTTP {status}")),
            interrupted: false,
            retryable: true,
            request_id: None,
            retry_after,
            retry_after_ms: None,
        }
    }

    /// Attach a request ID to this result for tracing.
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_result_ok() {
        let result = HttpResult::ok(200, serde_json::json!({"message": "hello"}));
        assert!(result.success);
        assert_eq!(result.status, Some(200));
        assert!(!result.interrupted);
        assert!(!result.retryable);
    }

    #[test]
    fn test_http_result_fail() {
        let result = HttpResult::fail("connection refused", true);
        assert!(!result.success);
        assert!(result.retryable);
        assert_eq!(result.error.as_deref(), Some("connection refused"));
    }

    #[test]
    fn test_http_result_interrupted() {
        let result = HttpResult::interrupted();
        assert!(!result.success);
        assert!(result.interrupted);
        assert!(!result.retryable);
    }

    #[test]
    fn test_http_result_retryable_status() {
        let result = HttpResult::retryable_status(429, None, None);
        assert!(!result.success);
        assert!(result.retryable);
        assert_eq!(result.status, Some(429));
    }

    #[test]
    fn test_http_result_retryable_status_with_retry_after() {
        let result = HttpResult::retryable_status(429, None, Some("30".to_string()));
        assert!(!result.success);
        assert!(result.retryable);
        assert_eq!(result.retry_after.as_deref(), Some("30"));
    }
}
