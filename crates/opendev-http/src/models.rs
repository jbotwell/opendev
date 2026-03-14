//! Shared types for HTTP operations.

use serde::{Deserialize, Serialize};

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
        }
    }

    /// Attach a request ID to this result for tracing.
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }
}

/// Configuration for retry behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (not counting the initial request).
    pub max_retries: u32,
    /// Base delays in seconds for exponential backoff.
    pub retry_delays: Vec<f64>,
    /// HTTP status codes that trigger a retry.
    pub retryable_status_codes: Vec<u16>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            retry_delays: vec![1.0, 2.0, 4.0],
            retryable_status_codes: vec![429, 503],
        }
    }
}

impl RetryConfig {
    /// Get the delay for a given attempt index (0-based).
    pub fn delay_for_attempt(&self, attempt: u32) -> std::time::Duration {
        let idx = (attempt as usize).min(self.retry_delays.len().saturating_sub(1));
        let secs = self.retry_delays.get(idx).copied().unwrap_or(4.0);
        std::time::Duration::from_secs_f64(secs)
    }

    /// Check if a status code is retryable.
    pub fn is_retryable_status(&self, status: u16) -> bool {
        self.retryable_status_codes.contains(&status)
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

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.retry_delays, vec![1.0, 2.0, 4.0]);
        assert!(config.is_retryable_status(429));
        assert!(config.is_retryable_status(503));
        assert!(!config.is_retryable_status(404));
    }

    #[test]
    fn test_retry_config_delay_for_attempt() {
        let config = RetryConfig::default();
        assert_eq!(
            config.delay_for_attempt(0),
            std::time::Duration::from_secs(1)
        );
        assert_eq!(
            config.delay_for_attempt(1),
            std::time::Duration::from_secs(2)
        );
        assert_eq!(
            config.delay_for_attempt(2),
            std::time::Duration::from_secs(4)
        );
        // Beyond bounds clamps to last
        assert_eq!(
            config.delay_for_attempt(99),
            std::time::Duration::from_secs(4)
        );
    }

    #[test]
    fn test_retry_config_serde_roundtrip() {
        let config = RetryConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: RetryConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.max_retries, config.max_retries);
        assert_eq!(deserialized.retry_delays, config.retry_delays);
    }
}
