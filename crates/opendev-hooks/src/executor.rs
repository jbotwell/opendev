//! Subprocess runner for hook commands.
//!
//! Hooks are executed as shell subprocesses. The command receives JSON on stdin
//! and communicates results via exit codes and optional JSON on stdout.
//!
//! Exit codes:
//! - 0: Success (operation proceeds)
//! - 2: Block (operation is denied)
//! - Other: Error (logged, operation proceeds)

use crate::models::HookCommand;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{error, warn};

/// Result from executing a single hook command.
#[derive(Debug, Clone, Default)]
pub struct HookResult {
    /// Process exit code (0 = success, 2 = block).
    pub exit_code: i32,
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
    /// Whether the command timed out.
    pub timed_out: bool,
    /// Error message if the command failed to execute.
    pub error: Option<String>,
}

impl HookResult {
    /// Hook succeeded (exit code 0, no timeout, no error).
    pub fn success(&self) -> bool {
        self.exit_code == 0 && !self.timed_out && self.error.is_none()
    }

    /// Hook requests blocking the operation (exit code 2).
    pub fn should_block(&self) -> bool {
        self.exit_code == 2
    }

    /// Parse stdout as a JSON object.
    ///
    /// Returns an empty map if stdout is empty or not valid JSON.
    pub fn parse_json_output(&self) -> HashMap<String, Value> {
        let trimmed = self.stdout.trim();
        if trimmed.is_empty() {
            return HashMap::new();
        }
        serde_json::from_str(trimmed).unwrap_or_default()
    }
}

/// Executes hook commands as subprocesses.
///
/// This is an async executor that spawns shell processes, pipes JSON on stdin,
/// and captures output with a timeout.
#[derive(Debug, Clone)]
pub struct HookExecutor;

impl HookExecutor {
    pub fn new() -> Self {
        Self
    }

    /// Execute a hook command.
    ///
    /// The command receives `stdin_data` as JSON on stdin and communicates
    /// results via exit code and optional JSON on stdout.
    pub async fn execute(&self, command: &HookCommand, stdin_data: &Value) -> HookResult {
        let stdin_json = match serde_json::to_string(stdin_data) {
            Ok(s) => s,
            Err(e) => {
                return HookResult {
                    exit_code: 1,
                    error: Some(format!("Failed to serialize stdin data: {e}")),
                    ..Default::default()
                };
            }
        };

        let timeout = Duration::from_secs(command.effective_timeout() as u64);

        // Determine shell to use
        let (shell, flag) = if cfg!(target_os = "windows") {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };

        let mut child = match Command::new(shell)
            .arg(flag)
            .arg(&command.command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                error!(
                    command = %command.command,
                    error = %e,
                    "Hook command failed to execute"
                );
                return HookResult {
                    exit_code: 1,
                    error: Some(format!("Failed to execute hook: {e}")),
                    ..Default::default()
                };
            }
        };

        // Write stdin
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(stdin_json.as_bytes()).await {
                warn!(error = %e, "Failed to write stdin to hook command");
            }
            // Drop stdin to close the pipe so the child can read EOF
            drop(stdin);
        }

        // Read stdout/stderr handles before waiting (wait_with_output takes
        // ownership, so we use the lower-level approach to allow killing on timeout).
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        // Wait with timeout
        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(Ok(status)) => {
                let exit_code = status.code().unwrap_or(1);

                // Read captured output
                let stdout = if let Some(mut out) = stdout_handle {
                    use tokio::io::AsyncReadExt;
                    let mut buf = Vec::new();
                    let _ = out.read_to_end(&mut buf).await;
                    String::from_utf8_lossy(&buf).to_string()
                } else {
                    String::new()
                };

                let stderr = if let Some(mut err) = stderr_handle {
                    use tokio::io::AsyncReadExt;
                    let mut buf = Vec::new();
                    let _ = err.read_to_end(&mut buf).await;
                    String::from_utf8_lossy(&buf).to_string()
                } else {
                    String::new()
                };

                HookResult {
                    exit_code,
                    stdout,
                    stderr,
                    timed_out: false,
                    error: None,
                }
            }
            Ok(Err(e)) => {
                error!(
                    command = %command.command,
                    error = %e,
                    "Hook command I/O error"
                );
                HookResult {
                    exit_code: 1,
                    error: Some(format!("Hook I/O error: {e}")),
                    ..Default::default()
                }
            }
            Err(_elapsed) => {
                warn!(
                    command = %command.command,
                    timeout_secs = command.effective_timeout(),
                    "Hook command timed out"
                );
                // Kill the child process
                let _ = child.kill().await;
                HookResult {
                    exit_code: 1,
                    timed_out: true,
                    error: Some(format!(
                        "Hook timed out after {}s",
                        command.effective_timeout()
                    )),
                    ..Default::default()
                }
            }
        }
    }
}

impl Default for HookExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_result_success() {
        let r = HookResult::default();
        assert!(r.success());
        assert!(!r.should_block());
    }

    #[test]
    fn test_hook_result_block() {
        let r = HookResult {
            exit_code: 2,
            ..Default::default()
        };
        assert!(!r.success());
        assert!(r.should_block());
    }

    #[test]
    fn test_hook_result_timeout() {
        let r = HookResult {
            exit_code: 1,
            timed_out: true,
            error: Some("timed out".into()),
            ..Default::default()
        };
        assert!(!r.success());
        assert!(!r.should_block());
    }

    #[test]
    fn test_hook_result_error() {
        let r = HookResult {
            exit_code: 0,
            error: Some("oops".into()),
            ..Default::default()
        };
        assert!(!r.success());
    }

    #[test]
    fn test_parse_json_output_valid() {
        let r = HookResult {
            stdout: r#"{"reason": "blocked", "decision": "deny"}"#.into(),
            ..Default::default()
        };
        let parsed = r.parse_json_output();
        assert_eq!(
            parsed.get("reason").and_then(|v| v.as_str()),
            Some("blocked")
        );
        assert_eq!(
            parsed.get("decision").and_then(|v| v.as_str()),
            Some("deny")
        );
    }

    #[test]
    fn test_parse_json_output_empty() {
        let r = HookResult::default();
        assert!(r.parse_json_output().is_empty());
    }

    #[test]
    fn test_parse_json_output_invalid() {
        let r = HookResult {
            stdout: "not json".into(),
            ..Default::default()
        };
        assert!(r.parse_json_output().is_empty());
    }

    #[tokio::test]
    async fn test_executor_echo_command() {
        let executor = HookExecutor::new();
        let cmd = HookCommand::new("echo hello");
        let stdin = serde_json::json!({"test": true});

        let result = executor.execute(&cmd, &stdin).await;
        assert!(result.success());
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_executor_reads_stdin() {
        let executor = HookExecutor::new();
        let cmd = HookCommand::new("cat");
        let stdin = serde_json::json!({"key": "value"});

        let result = executor.execute(&cmd, &stdin).await;
        assert!(result.success());
        let parsed: serde_json::Value = serde_json::from_str(result.stdout.trim()).unwrap();
        assert_eq!(parsed["key"], "value");
    }

    #[tokio::test]
    async fn test_executor_exit_code_2_blocks() {
        let executor = HookExecutor::new();
        let cmd = HookCommand::new("exit 2");
        let stdin = serde_json::json!({});

        let result = executor.execute(&cmd, &stdin).await;
        assert!(result.should_block());
        assert!(!result.success());
    }

    #[tokio::test]
    async fn test_executor_nonzero_exit() {
        let executor = HookExecutor::new();
        let cmd = HookCommand::new("exit 1");
        let stdin = serde_json::json!({});

        let result = executor.execute(&cmd, &stdin).await;
        assert!(!result.success());
        assert!(!result.should_block());
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_executor_timeout() {
        let executor = HookExecutor::new();
        let cmd = HookCommand::with_timeout("sleep 60", 1);
        let stdin = serde_json::json!({});

        let result = executor.execute(&cmd, &stdin).await;
        assert!(result.timed_out);
        assert!(!result.success());
    }

    #[tokio::test]
    async fn test_executor_invalid_command() {
        let executor = HookExecutor::new();
        let cmd = HookCommand::new("__nonexistent_command_xyz_12345__");
        let stdin = serde_json::json!({});

        let result = executor.execute(&cmd, &stdin).await;
        // The shell will report an error (command not found) with non-zero exit
        assert!(!result.success());
    }

    #[tokio::test]
    async fn test_executor_captures_stderr() {
        let executor = HookExecutor::new();
        let cmd = HookCommand::new("echo err >&2");
        let stdin = serde_json::json!({});

        let result = executor.execute(&cmd, &stdin).await;
        assert!(result.success());
        assert_eq!(result.stderr.trim(), "err");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_executor_json_stdout() {
        let executor = HookExecutor::new();
        let cmd = HookCommand::new(r#"echo '{"additionalContext":"extra info"}'"#);
        let stdin = serde_json::json!({});

        let result = executor.execute(&cmd, &stdin).await;
        assert!(result.success());
        let parsed = result.parse_json_output();
        assert_eq!(
            parsed.get("additionalContext").and_then(|v| v.as_str()),
            Some("extra info")
        );
    }
}
