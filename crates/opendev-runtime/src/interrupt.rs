//! Centralized interrupt/cancellation token for a single agent run.
//!
//! One token is created per user query execution. All components (LLM caller,
//! tool executor, HTTP client, etc.) share the same token so that a single
//! ESC press reliably cancels the entire operation.
//!
//! Uses `tokio_util::sync::CancellationToken` under the hood for true
//! async-safe cancellation, plus an `AtomicBool` for synchronous polling.
//!
//! Ported from `opendev/core/runtime/interrupt_token.py`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_util::sync::CancellationToken;

/// Async-safe cancellation token shared across all components of a run.
///
/// # Usage
///
/// ```rust
/// # use opendev_runtime::InterruptToken;
/// let token = InterruptToken::new();
/// let child = token.clone();
///
/// // In the agent thread
/// if child.is_requested() {
///     // bail out
/// }
///
/// // In the UI thread on ESC
/// token.request();
/// ```
#[derive(Clone)]
pub struct InterruptToken {
    inner: Arc<InterruptInner>,
}

struct InterruptInner {
    /// Synchronous flag for polling-based checks.
    flag: AtomicBool,
    /// Soft yield flag for backgrounding — does NOT cancel the CancellationToken.
    background: AtomicBool,
    /// Tokio cancellation token for async `.cancelled()` futures.
    cancel: CancellationToken,
}

impl InterruptToken {
    /// Create a new un-triggered token.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(InterruptInner {
                flag: AtomicBool::new(false),
                background: AtomicBool::new(false),
                cancel: CancellationToken::new(),
            }),
        }
    }

    /// Signal that the user wants to cancel the current operation.
    ///
    /// This is cheap and idempotent — calling it multiple times is fine.
    pub fn request(&self) {
        self.inner.flag.store(true, Ordering::Release);
        self.inner.cancel.cancel();
    }

    /// Force-interrupt the current operation.
    ///
    /// In the Rust implementation this is identical to `request()` since we rely
    /// on cooperative cancellation via the `CancellationToken` rather than
    /// injecting async exceptions into threads (which is a CPython-specific trick).
    pub fn force_interrupt(&self) {
        self.request();
    }

    /// Request that the current operation be moved to the background.
    ///
    /// This cancels the `CancellationToken` to immediately interrupt any
    /// in-flight async operation (LLM streaming, tool execution), but does
    /// NOT set the hard interrupt `flag`. The react loop distinguishes
    /// background from hard interrupt by checking `is_background_requested()`.
    pub fn request_background(&self) {
        self.inner.background.store(true, Ordering::Release);
        self.inner.cancel.cancel();
    }

    /// Check whether backgrounding has been requested.
    pub fn is_background_requested(&self) -> bool {
        self.inner.background.load(Ordering::Acquire)
    }

    /// Check whether cancellation has been requested.
    ///
    /// This is a cheap atomic load suitable for hot polling loops.
    pub fn is_requested(&self) -> bool {
        self.inner.flag.load(Ordering::Acquire)
    }

    /// Return an error if cancellation was requested.
    pub fn throw_if_requested(&self) -> Result<(), InterruptedError> {
        if self.is_requested() {
            Err(InterruptedError)
        } else {
            Ok(())
        }
    }

    /// Wait until cancellation is requested.
    ///
    /// This is the primary async integration point — select! against this
    /// alongside your actual work future.
    pub async fn cancelled(&self) {
        self.inner.cancel.cancelled().await;
    }

    /// Get the underlying `tokio_util::sync::CancellationToken`.
    ///
    /// Useful when you need to pass a token to lower-level async code
    /// or create child tokens.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.inner.cancel
    }

    /// Create a child token that is cancelled when the parent is cancelled.
    pub fn child_token(&self) -> CancellationToken {
        self.inner.cancel.child_token()
    }

    /// Clear the cancellation signal (use with care — mainly for token reuse
    /// across multiple agent runs).
    pub fn reset(&self) {
        self.inner.flag.store(false, Ordering::Release);
        // Note: CancellationToken cannot be un-cancelled. For multi-run reuse
        // the caller should create a new InterruptToken instead.
    }

    // ------------------------------------------------------------------
    // TaskMonitor compatibility
    // ------------------------------------------------------------------

    /// Alias for `is_requested()` — TaskMonitor compatibility.
    pub fn should_interrupt(&self) -> bool {
        self.is_requested()
    }

    /// Alias for `request()` — TaskMonitor compatibility.
    pub fn request_interrupt(&self) {
        self.request();
    }
}

impl Default for InterruptToken {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for InterruptToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InterruptToken")
            .field("requested", &self.is_requested())
            .finish()
    }
}

/// Error raised when an operation is interrupted by the user.
#[derive(Debug, Clone, thiserror::Error)]
#[error("Interrupted by user")]
pub struct InterruptedError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_token_not_requested() {
        let token = InterruptToken::new();
        assert!(!token.is_requested());
        assert!(!token.should_interrupt());
    }

    #[test]
    fn test_request_sets_flag() {
        let token = InterruptToken::new();
        token.request();
        assert!(token.is_requested());
    }

    #[test]
    fn test_throw_if_requested_ok() {
        let token = InterruptToken::new();
        assert!(token.throw_if_requested().is_ok());
    }

    #[test]
    fn test_throw_if_requested_err() {
        let token = InterruptToken::new();
        token.request();
        assert!(token.throw_if_requested().is_err());
    }

    #[test]
    fn test_clone_shares_state() {
        let token = InterruptToken::new();
        let clone = token.clone();
        assert!(!clone.is_requested());
        token.request();
        assert!(clone.is_requested());
    }

    #[test]
    fn test_force_interrupt() {
        let token = InterruptToken::new();
        token.force_interrupt();
        assert!(token.is_requested());
    }

    #[test]
    fn test_reset() {
        let token = InterruptToken::new();
        token.request();
        assert!(token.is_requested());
        token.reset();
        assert!(!token.is_requested());
    }

    #[test]
    fn test_request_background_sets_flag() {
        let token = InterruptToken::new();
        assert!(!token.is_background_requested());
        token.request_background();
        assert!(token.is_background_requested());
    }

    #[test]
    fn test_request_background_cancels_but_no_hard_flag() {
        let token = InterruptToken::new();
        token.request_background();
        assert!(token.is_background_requested());
        // Should cancel the CancellationToken (to interrupt in-flight ops)
        // but NOT set the hard interrupt flag
        assert!(!token.is_requested());
        assert!(token.cancellation_token().is_cancelled());
    }

    #[test]
    fn test_background_clone_shares_state() {
        let token = InterruptToken::new();
        let clone = token.clone();
        token.request_background();
        assert!(clone.is_background_requested());
    }

    #[test]
    fn test_request_idempotent() {
        let token = InterruptToken::new();
        token.request();
        token.request();
        token.request();
        assert!(token.is_requested());
    }

    #[test]
    fn test_debug_format() {
        let token = InterruptToken::new();
        let debug = format!("{:?}", token);
        assert!(debug.contains("InterruptToken"));
        assert!(debug.contains("false"));
    }

    #[tokio::test]
    async fn test_cancelled_future() {
        let token = InterruptToken::new();
        let token2 = token.clone();

        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            token2.request();
        });

        // Should resolve once token is requested
        token.cancelled().await;
        assert!(token.is_requested());
    }

    #[tokio::test]
    async fn test_select_with_cancelled() {
        let token = InterruptToken::new();
        let token2 = token.clone();

        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            token2.request();
        });

        let result = tokio::select! {
            _ = token.cancelled() => "interrupted",
            _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => "timeout",
        };

        assert_eq!(result, "interrupted");
    }

    #[test]
    fn test_child_token() {
        let token = InterruptToken::new();
        let child = token.child_token();
        assert!(!child.is_cancelled());
        token.request();
        assert!(child.is_cancelled());
    }
}
