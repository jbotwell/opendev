//! Hook manager — orchestrates hook execution for lifecycle events.
//!
//! Takes a snapshot of [`HookConfig`] at construction time. Mid-session changes
//! to settings.json are not reflected (security: prevents config TOCTOU).

use crate::executor::{HookExecutor, HookResult};
use crate::models::{HookConfig, HookEvent};
use serde_json::{Map, Value};
use tracing::warn;

/// Aggregated outcome from running all hooks for an event.
#[derive(Debug, Clone, Default)]
pub struct HookOutcome {
    /// Whether any hook requested blocking the operation.
    pub blocked: bool,
    /// Human-readable reason for the block.
    pub block_reason: String,
    /// Individual results from each hook command.
    pub results: Vec<HookResult>,
    /// Additional context injected by a hook (appended to tool output).
    pub additional_context: Option<String>,
    /// Updated input provided by a hook (replaces tool input).
    pub updated_input: Option<Value>,
    /// Permission decision from a hook (e.g., "allow", "deny").
    pub permission_decision: Option<String>,
    /// General decision string from a hook.
    pub decision: Option<String>,
}

impl HookOutcome {
    /// Whether all hooks passed without blocking.
    pub fn allowed(&self) -> bool {
        !self.blocked
    }
}

/// Orchestrates hook execution for lifecycle events.
///
/// The manager holds a frozen snapshot of the hook configuration, an executor
/// for running subprocess commands, and session metadata used to build stdin
/// payloads for hook commands.
pub struct HookManager {
    config: HookConfig,
    session_id: String,
    cwd: String,
    executor: HookExecutor,
}

impl HookManager {
    /// Create a new hook manager.
    ///
    /// The `config` should already have `compile_all()` and
    /// `strip_unknown_events()` called on it.
    pub fn new(config: HookConfig, session_id: impl Into<String>, cwd: impl Into<String>) -> Self {
        Self {
            config,
            session_id: session_id.into(),
            cwd: cwd.into(),
            executor: HookExecutor::new(),
        }
    }

    /// Create a manager with no hooks configured (no-op for all events).
    pub fn noop() -> Self {
        Self::new(HookConfig::empty(), "", "")
    }

    /// Fast check: are there hooks registered for this event?
    pub fn has_hooks_for(&self, event: HookEvent) -> bool {
        self.config.has_hooks_for(event)
    }

    /// Run all matching hooks for an event.
    ///
    /// Hooks execute sequentially. Short-circuits on block (exit code 2).
    ///
    /// # Arguments
    /// - `event`: The lifecycle event.
    /// - `match_value`: Value to test against matcher regex (e.g., tool name).
    /// - `event_data`: Additional event-specific data for the stdin payload.
    pub async fn run_hooks(
        &self,
        event: HookEvent,
        match_value: Option<&str>,
        event_data: Option<&Value>,
    ) -> HookOutcome {
        let mut outcome = HookOutcome::default();

        let matchers = self.config.get_matchers(event);
        if matchers.is_empty() {
            return outcome;
        }

        for matcher in matchers {
            if !matcher.matches(match_value) {
                continue;
            }

            let stdin_data = self.build_stdin(event, match_value, event_data);

            for command in &matcher.hooks {
                let result = self.executor.execute(command, &stdin_data).await;

                if result.should_block() {
                    let parsed = result.parse_json_output();
                    outcome.block_reason = parsed
                        .get("reason")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| {
                            let stderr = result.stderr.trim();
                            if stderr.is_empty() {
                                "Blocked by hook".to_string()
                            } else {
                                stderr.to_string()
                            }
                        });
                    outcome.decision = parsed
                        .get("decision")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    outcome.blocked = true;
                    outcome.results.push(result);
                    return outcome;
                }

                if result.success() {
                    let parsed = result.parse_json_output();
                    if let Some(ctx) = parsed.get("additionalContext").and_then(|v| v.as_str()) {
                        outcome.additional_context = Some(ctx.to_string());
                    }
                    if let Some(input) = parsed.get("updatedInput") {
                        outcome.updated_input = Some(input.clone());
                    }
                    if let Some(perm) = parsed.get("permissionDecision").and_then(|v| v.as_str()) {
                        outcome.permission_decision = Some(perm.to_string());
                    }
                    if let Some(dec) = parsed.get("decision").and_then(|v| v.as_str()) {
                        outcome.decision = Some(dec.to_string());
                    }
                } else if let Some(ref err) = result.error {
                    warn!(
                        event = %event,
                        error = %err,
                        "Hook command error"
                    );
                }

                outcome.results.push(result);
            }
        }

        outcome
    }

    /// Fire-and-forget hook execution.
    ///
    /// Spawns hook execution as a background tokio task. Used for events
    /// where we don't need to wait for the result (e.g., PostToolUse logging).
    pub fn run_hooks_async(
        &self,
        event: HookEvent,
        match_value: Option<String>,
        event_data: Option<Value>,
    ) where
        Self: Send + Sync + 'static,
    {
        if !self.has_hooks_for(event) {
            return;
        }

        // Clone what we need for the spawned task
        let config = self.config.clone();
        let session_id = self.session_id.clone();
        let cwd = self.cwd.clone();

        tokio::spawn(async move {
            let manager = HookManager::new(config, session_id, cwd);
            let _ = manager
                .run_hooks(event, match_value.as_deref(), event_data.as_ref())
                .await;
        });
    }

    /// Build the JSON payload sent to hook commands on stdin.
    ///
    /// Follows the hook protocol:
    /// - `session_id`: Current session ID
    /// - `cwd`: Current working directory
    /// - `hook_event_name`: The event name (e.g., "PreToolUse")
    /// - `tool_name`: Tool name (for tool events)
    /// - `agent_type`: Agent type (for subagent events)
    /// - `startup_type`: Startup type (for SessionStart)
    /// - `trigger`: Trigger type (for PreCompact)
    /// - Additional fields from `event_data`
    fn build_stdin(
        &self,
        event: HookEvent,
        match_value: Option<&str>,
        event_data: Option<&Value>,
    ) -> Value {
        let mut payload = Map::new();

        payload.insert(
            "session_id".to_string(),
            Value::String(self.session_id.clone()),
        );
        payload.insert("cwd".to_string(), Value::String(self.cwd.clone()));
        payload.insert(
            "hook_event_name".to_string(),
            Value::String(event.as_str().to_string()),
        );

        let mv = match_value.unwrap_or("");

        // Tool events include tool_name
        if event.is_tool_event() {
            payload.insert("tool_name".to_string(), Value::String(mv.to_string()));
        }

        // Subagent events include agent_type
        if event.is_subagent_event() {
            payload.insert("agent_type".to_string(), Value::String(mv.to_string()));
        }

        // SessionStart includes startup_type
        if event == HookEvent::SessionStart {
            payload.insert(
                "startup_type".to_string(),
                Value::String(if mv.is_empty() { "startup" } else { mv }.to_string()),
            );
        }

        // PreCompact includes trigger
        if event == HookEvent::PreCompact {
            payload.insert(
                "trigger".to_string(),
                Value::String(if mv.is_empty() { "auto" } else { mv }.to_string()),
            );
        }

        // Merge event-specific data
        if let Some(Value::Object(data)) = event_data {
            // Standard fields first
            for key in &[
                "tool_input",
                "tool_response",
                "user_prompt",
                "agent_task",
                "agent_result",
            ] {
                if let Some(val) = data.get(*key) {
                    payload.insert((*key).to_string(), val.clone());
                }
            }
            // Pass through any other data not already in payload
            for (key, val) in data {
                if !payload.contains_key(key) {
                    payload.insert(key.clone(), val.clone());
                }
            }
        }

        Value::Object(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{HookCommand, HookMatcher};
    use serde_json::json;

    fn make_config_with_echo(event: HookEvent, pattern: Option<&str>, cmd: &str) -> HookConfig {
        let mut config = HookConfig::empty();
        let matcher = match pattern {
            Some(p) => HookMatcher::with_pattern(p, vec![HookCommand::new(cmd)]),
            None => HookMatcher::catch_all(vec![HookCommand::new(cmd)]),
        };
        config.add_matcher(event, matcher);
        config
    }

    #[test]
    fn test_noop_manager() {
        let manager = HookManager::noop();
        assert!(!manager.has_hooks_for(HookEvent::PreToolUse));
        assert!(!manager.has_hooks_for(HookEvent::SessionStart));
    }

    #[test]
    fn test_has_hooks_for() {
        let config = make_config_with_echo(HookEvent::PreToolUse, None, "echo test");
        let manager = HookManager::new(config, "sess-1", "/tmp");
        assert!(manager.has_hooks_for(HookEvent::PreToolUse));
        assert!(!manager.has_hooks_for(HookEvent::PostToolUse));
    }

    #[tokio::test]
    async fn test_run_hooks_no_matchers() {
        let manager = HookManager::noop();
        let outcome = manager
            .run_hooks(HookEvent::PreToolUse, Some("bash"), None)
            .await;
        assert!(outcome.allowed());
        assert!(outcome.results.is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_hooks_success() {
        let config = make_config_with_echo(HookEvent::PreToolUse, None, "echo ok");
        let manager = HookManager::new(config, "sess-1", "/tmp");
        let outcome = manager
            .run_hooks(HookEvent::PreToolUse, Some("bash"), None)
            .await;
        assert!(outcome.allowed());
        assert_eq!(outcome.results.len(), 1);
        assert!(outcome.results[0].success());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_hooks_blocked() {
        let config = make_config_with_echo(
            HookEvent::PreToolUse,
            None,
            r#"echo '{"reason":"dangerous"}' && exit 2"#,
        );
        let manager = HookManager::new(config, "sess-1", "/tmp");
        let outcome = manager
            .run_hooks(HookEvent::PreToolUse, Some("bash"), None)
            .await;
        assert!(outcome.blocked);
        assert_eq!(outcome.block_reason, "dangerous");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_hooks_block_stderr_fallback() {
        let config = make_config_with_echo(
            HookEvent::PreToolUse,
            None,
            "echo 'not blocked' >&2; exit 2",
        );
        let manager = HookManager::new(config, "sess-1", "/tmp");
        let outcome = manager
            .run_hooks(HookEvent::PreToolUse, Some("bash"), None)
            .await;
        assert!(outcome.blocked);
        assert_eq!(outcome.block_reason, "not blocked");
    }

    #[tokio::test]
    async fn test_run_hooks_matcher_filters() {
        let config = make_config_with_echo(HookEvent::PreToolUse, Some(r"^bash$"), "exit 2");
        let manager = HookManager::new(config, "sess-1", "/tmp");

        // "bash" matches the pattern -> blocked
        let outcome = manager
            .run_hooks(HookEvent::PreToolUse, Some("bash"), None)
            .await;
        assert!(outcome.blocked);

        // "edit" does not match -> allowed, no hooks run
        let outcome = manager
            .run_hooks(HookEvent::PreToolUse, Some("edit"), None)
            .await;
        assert!(outcome.allowed());
        assert!(outcome.results.is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_hooks_additional_context() {
        let config = make_config_with_echo(
            HookEvent::PostToolUse,
            None,
            r#"echo '{"additionalContext":"extra info"}'"#,
        );
        let manager = HookManager::new(config, "sess-1", "/tmp");
        let outcome = manager
            .run_hooks(HookEvent::PostToolUse, Some("bash"), None)
            .await;
        assert!(outcome.allowed());
        assert_eq!(outcome.additional_context.as_deref(), Some("extra info"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_hooks_permission_decision() {
        let config = make_config_with_echo(
            HookEvent::PreToolUse,
            None,
            r#"echo '{"permissionDecision":"allow"}'"#,
        );
        let manager = HookManager::new(config, "sess-1", "/tmp");
        let outcome = manager
            .run_hooks(HookEvent::PreToolUse, Some("bash"), None)
            .await;
        assert!(outcome.allowed());
        assert_eq!(outcome.permission_decision.as_deref(), Some("allow"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_hooks_updated_input() {
        let config = make_config_with_echo(
            HookEvent::PreToolUse,
            None,
            r#"echo '{"updatedInput":{"command":"ls -la"}}'"#,
        );
        let manager = HookManager::new(config, "sess-1", "/tmp");
        let outcome = manager
            .run_hooks(HookEvent::PreToolUse, Some("bash"), None)
            .await;
        assert!(outcome.allowed());
        let updated = outcome.updated_input.unwrap();
        assert_eq!(updated["command"], "ls -la");
    }

    #[tokio::test]
    async fn test_run_hooks_multiple_commands_short_circuit() {
        // First command blocks -> second should not run
        let mut config = HookConfig::empty();
        let matcher = HookMatcher::catch_all(vec![
            HookCommand::new("exit 2"),
            HookCommand::new("echo should-not-run"),
        ]);
        config.add_matcher(HookEvent::PreToolUse, matcher);

        let manager = HookManager::new(config, "sess-1", "/tmp");
        let outcome = manager
            .run_hooks(HookEvent::PreToolUse, Some("bash"), None)
            .await;
        assert!(outcome.blocked);
        // Only one result because we short-circuited
        assert_eq!(outcome.results.len(), 1);
    }

    #[tokio::test]
    async fn test_run_hooks_error_continues() {
        // A failing command (non-zero, non-2 exit) should not block
        let config = make_config_with_echo(HookEvent::PostToolUse, None, "exit 1");
        let manager = HookManager::new(config, "sess-1", "/tmp");
        let outcome = manager
            .run_hooks(HookEvent::PostToolUse, Some("bash"), None)
            .await;
        assert!(outcome.allowed());
        assert_eq!(outcome.results.len(), 1);
        assert!(!outcome.results[0].success());
    }

    #[tokio::test]
    async fn test_build_stdin_tool_event() {
        let config = make_config_with_echo(
            HookEvent::PreToolUse,
            None,
            "cat", // echo stdin back
        );
        let manager = HookManager::new(config, "sess-42", "/home/user");

        let event_data = json!({
            "tool_input": {"command": "ls"},
            "extra_field": "value"
        });

        let outcome = manager
            .run_hooks(HookEvent::PreToolUse, Some("bash"), Some(&event_data))
            .await;

        assert!(outcome.allowed());
        let stdout = &outcome.results[0].stdout;
        let parsed: Value = serde_json::from_str(stdout).unwrap();

        assert_eq!(parsed["session_id"], "sess-42");
        assert_eq!(parsed["cwd"], "/home/user");
        assert_eq!(parsed["hook_event_name"], "PreToolUse");
        assert_eq!(parsed["tool_name"], "bash");
        assert_eq!(parsed["tool_input"]["command"], "ls");
        assert_eq!(parsed["extra_field"], "value");
    }

    #[tokio::test]
    async fn test_build_stdin_session_start() {
        let config = make_config_with_echo(HookEvent::SessionStart, None, "cat");
        let manager = HookManager::new(config, "sess-1", "/tmp");

        let outcome = manager
            .run_hooks(HookEvent::SessionStart, Some("resume"), None)
            .await;

        let stdout = &outcome.results[0].stdout;
        let parsed: Value = serde_json::from_str(stdout).unwrap();
        assert_eq!(parsed["startup_type"], "resume");
        assert_eq!(parsed["hook_event_name"], "SessionStart");
    }

    #[tokio::test]
    async fn test_build_stdin_session_start_default() {
        let config = make_config_with_echo(HookEvent::SessionStart, None, "cat");
        let manager = HookManager::new(config, "sess-1", "/tmp");

        let outcome = manager.run_hooks(HookEvent::SessionStart, None, None).await;

        let stdout = &outcome.results[0].stdout;
        let parsed: Value = serde_json::from_str(stdout).unwrap();
        // Default startup_type when match_value is None
        assert_eq!(parsed["startup_type"], "startup");
    }

    #[tokio::test]
    async fn test_build_stdin_subagent_event() {
        let config = make_config_with_echo(HookEvent::SubagentStart, None, "cat");
        let manager = HookManager::new(config, "sess-1", "/tmp");

        let outcome = manager
            .run_hooks(HookEvent::SubagentStart, Some("code_explorer"), None)
            .await;

        let stdout = &outcome.results[0].stdout;
        let parsed: Value = serde_json::from_str(stdout).unwrap();
        assert_eq!(parsed["agent_type"], "code_explorer");
    }

    #[tokio::test]
    async fn test_build_stdin_pre_compact() {
        let config = make_config_with_echo(HookEvent::PreCompact, None, "cat");
        let manager = HookManager::new(config, "sess-1", "/tmp");

        let outcome = manager
            .run_hooks(HookEvent::PreCompact, Some("manual"), None)
            .await;

        let stdout = &outcome.results[0].stdout;
        let parsed: Value = serde_json::from_str(stdout).unwrap();
        assert_eq!(parsed["trigger"], "manual");
    }

    #[tokio::test]
    async fn test_multiple_matchers_both_run() {
        let mut config = HookConfig::empty();
        // Two catch-all matchers, each with one command
        config.add_matcher(
            HookEvent::PostToolUse,
            HookMatcher::catch_all(vec![HookCommand::new("echo first")]),
        );
        config.add_matcher(
            HookEvent::PostToolUse,
            HookMatcher::catch_all(vec![HookCommand::new("echo second")]),
        );

        let manager = HookManager::new(config, "sess-1", "/tmp");
        let outcome = manager
            .run_hooks(HookEvent::PostToolUse, Some("bash"), None)
            .await;

        assert!(outcome.allowed());
        assert_eq!(outcome.results.len(), 2);
        assert_eq!(outcome.results[0].stdout.trim(), "first");
        assert_eq!(outcome.results[1].stdout.trim(), "second");
    }
}
