//! Utility methods on ReactLoop: response processing, tool classification, metrics.

use serde_json::Value;
use tracing::{debug, warn};

use crate::traits::LlmResponse;

use super::ReactLoop;
use super::types::{IterationMetrics, TurnResult};

impl ReactLoop {
    /// Update per-query thinking context (original task and system prompt).
    ///
    /// Call this before each `run()` to set the user's original task text
    /// and the pre-composed thinking system prompt.
    pub fn set_thinking_context(
        &mut self,
        original_task: Option<String>,
        thinking_system_prompt: Option<String>,
    ) {
        self.config.original_task = original_task;
        self.config.thinking_system_prompt = thinking_system_prompt;
    }

    /// Return a snapshot of accumulated iteration metrics collected during `run()`.
    pub fn iteration_metrics(&self) -> Vec<IterationMetrics> {
        self.iteration_metrics.lock().unwrap().clone()
    }

    /// Clear accumulated iteration metrics.
    pub fn clear_metrics(&self) {
        self.iteration_metrics.lock().unwrap().clear();
    }

    /// Push a new iteration metrics entry.
    pub(super) fn push_metrics(&self, metrics: IterationMetrics) {
        self.iteration_metrics.lock().unwrap().push(metrics);
    }

    /// Process a single LLM response and determine the next action.
    ///
    /// This is the core decision function of the ReAct loop. It examines
    /// the LLM response and returns a `TurnResult` indicating what should
    /// happen next.
    pub fn process_response(
        &self,
        response: &LlmResponse,
        consecutive_no_tool_calls: usize,
    ) -> TurnResult {
        if response.interrupted {
            return TurnResult::Interrupted;
        }

        if !response.success {
            // Failed API call — if we have an error, treat as needing continuation
            warn!(
                error = response.error.as_deref().unwrap_or("unknown"),
                "LLM call failed"
            );
            return TurnResult::Continue;
        }

        // Check for tool calls
        let tool_calls = response.tool_calls.as_ref().and_then(|tcs| {
            if tcs.is_empty() {
                None
            } else {
                Some(tcs.clone())
            }
        });

        match tool_calls {
            Some(tcs) => TurnResult::ToolCall { tool_calls: tcs },
            None => {
                // No tool calls — check if we should accept completion
                let content = response.content.as_deref().unwrap_or("Done.").to_string();

                if consecutive_no_tool_calls >= self.config.max_nudge_attempts {
                    debug!("Max nudge attempts reached, accepting completion");
                    TurnResult::Complete {
                        content,
                        status: None,
                    }
                } else {
                    // Still have nudge budget — caller decides whether to nudge
                    TurnResult::Complete {
                        content,
                        status: None,
                    }
                }
            }
        }
    }

    /// Check if a set of tool calls are all parallelizable.
    pub fn all_parallelizable(&self, tool_calls: &[Value]) -> bool {
        if tool_calls.len() <= 1 {
            return false;
        }

        tool_calls.iter().all(|tc| {
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            self.parallelizable.contains(name) && name != "task_complete"
        })
    }

    /// Check if a tool call is for task completion.
    pub fn is_task_complete(tool_call: &Value) -> bool {
        tool_call
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
            == Some("task_complete")
    }

    /// Extract the summary and status from a task_complete tool call.
    pub fn extract_task_complete_args(tool_call: &Value) -> (String, String) {
        let args_str = tool_call
            .get("function")
            .and_then(|f| f.get("arguments"))
            .and_then(|a| a.as_str())
            .unwrap_or("{}");

        let args: Value = serde_json::from_str(args_str).unwrap_or_default();
        let summary = args
            .get("result")
            .or_else(|| args.get("summary"))
            .and_then(|s| s.as_str())
            .unwrap_or("Task completed")
            .to_string();
        let status = args
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("success")
            .to_string();

        (summary, status)
    }

    /// Format a tool execution result into a string for the message history.
    pub fn format_tool_result(tool_name: &str, result: &Value) -> String {
        let success = result
            .get("success")
            .and_then(|s| s.as_bool())
            .unwrap_or(false);

        let base = if success {
            let output = result
                .get("separate_response")
                .or_else(|| result.get("output"))
                .and_then(|o| o.as_str())
                .unwrap_or("");

            let completion_status = result.get("completion_status").and_then(|s| s.as_str());

            if let Some(status) = completion_status {
                format!("[completion_status={status}]\n{output}")
            } else {
                output.to_string()
            }
        } else {
            let error = result
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("Tool execution failed");
            format!("Error in {tool_name}: {error}")
        };

        // Append LLM-only suffix if present (hidden from UI, visible to LLM)
        if let Some(suffix) = result.get("llm_suffix").and_then(|s| s.as_str()) {
            format!("{base}\n\n{suffix}")
        } else {
            base
        }
    }

    /// Classify an error for targeted nudge selection.
    pub fn classify_error(error_text: &str) -> &'static str {
        let lower = error_text.to_lowercase();
        if lower.contains("permission denied") {
            "permission_error"
        } else if lower.contains("old_content") || lower.contains("old content") {
            "edit_mismatch"
        } else if lower.contains("no such file") || lower.contains("not found") {
            "file_not_found"
        } else if lower.contains("syntax") {
            "syntax_error"
        } else if lower.contains("429") || lower.contains("rate limit") {
            "rate_limit"
        } else if lower.contains("timeout") || lower.contains("timed out") {
            "timeout"
        } else {
            "generic"
        }
    }

    /// Check if the iteration limit has been reached.
    pub fn check_iteration_limit(&self, iteration: usize) -> bool {
        match self.config.max_iterations {
            Some(max) => iteration > max,
            None => false,
        }
    }

    /// Check if the last tool calls were all read-only and succeeded.
    ///
    /// Used to skip the thinking phase when the previous turn only did
    /// information gathering (no state changes to re-plan around).
    /// Mirrors Python's `IterationMixin._last_tools_were_readonly()`.
    pub fn should_skip_thinking(&self, messages: &[Value]) -> bool {
        let mut found_tools = false;
        // Collect tool names from the most recent assistant tool_calls
        let _last_assistant_tools: Vec<String> = Vec::new();

        for msg in messages.iter().rev() {
            // Skip injected thinking trace messages (fake assistant-user pair)
            if msg.get("_thinking").and_then(|v| v.as_bool()) == Some(true) {
                continue;
            }

            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            match role {
                "tool" => {
                    let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    let tool_name = msg.get("name").and_then(|n| n.as_str()).unwrap_or("");

                    // If any tool errored, don't skip thinking
                    if content.starts_with("Error")
                        || content.to_lowercase().contains("\"success\": false")
                    {
                        return false;
                    }
                    if !tool_name.is_empty() && !self.readonly_tools.contains(tool_name) {
                        return false;
                    }
                    found_tools = true;
                }
                "assistant" if found_tools => {
                    // Check tool_calls in the assistant message for non-readonly tools
                    if let Some(tcs) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                        for tc in tcs {
                            if let Some(name) = tc
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|n| n.as_str())
                                && !self.readonly_tools.contains(name)
                            {
                                return false;
                            }
                        }
                    }
                    break;
                }
                "user" if found_tools => break,
                "user" | "assistant" => return false,
                _ => {}
            }
        }
        found_tools
    }

    /// Count the number of assistant messages with tool_calls in a subagent result.
    ///
    /// Used for shallow subagent detection. If a subagent only made <=1 tool
    /// call, the parent could have done it directly.
    pub fn count_subagent_tool_calls(messages: &[Value]) -> usize {
        messages
            .iter()
            .filter(|msg| {
                msg.get("role").and_then(|r| r.as_str()) == Some("assistant")
                    && msg.get("tool_calls").is_some()
                    && !msg
                        .get("tool_calls")
                        .and_then(|tc| tc.as_array())
                        .map(|a| a.is_empty())
                        .unwrap_or(true)
            })
            .count()
    }

    /// Generate a shallow subagent warning suffix if applicable.
    ///
    /// Returns `Some(warning)` if the subagent made <=1 tool calls, `None` otherwise.
    pub fn shallow_subagent_warning(result_messages: &[Value], success: bool) -> Option<String> {
        if !success {
            return None;
        }
        let tool_call_count = Self::count_subagent_tool_calls(result_messages);
        if tool_call_count <= 1 {
            Some(format!(
                "\n\n[SHALLOW SUBAGENT WARNING] This subagent only made \
                 {tool_call_count} tool call(s). Spawning a subagent for a task \
                 that requires ≤1 tool call is wasteful — you should have used a \
                 direct tool call instead. For future similar tasks, use read_file, \
                 search, or list_files directly rather than spawning a subagent."
            ))
        } else {
            None
        }
    }

    /// Process a single iteration given an already-parsed LLM response.
    ///
    /// This is the preferred integration point. The caller makes the HTTP
    /// request, parses the response, then calls this method to determine
    /// the next action.
    pub fn process_iteration(
        &self,
        response: &LlmResponse,
        messages: &mut Vec<Value>,
        iteration: usize,
        consecutive_no_tool_calls: &mut usize,
    ) -> Result<TurnResult, crate::traits::AgentError> {
        if self.check_iteration_limit(iteration) {
            return Ok(TurnResult::MaxIterations);
        }

        if response.interrupted {
            return Ok(TurnResult::Interrupted);
        }

        if !response.success {
            return Err(crate::traits::AgentError::LlmError(
                response
                    .error
                    .clone()
                    .unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }

        // Append assistant message to history
        if let Some(ref msg) = response.message {
            let raw_content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
            let mut assistant_msg = serde_json::json!({
                "role": "assistant",
                "content": raw_content,
            });
            if let Some(tool_calls) = msg.get("tool_calls")
                && !tool_calls.is_null()
            {
                assistant_msg["tool_calls"] = tool_calls.clone();
            }
            messages.push(assistant_msg);
        }

        let turn = self.process_response(response, *consecutive_no_tool_calls);

        match &turn {
            TurnResult::ToolCall { .. } => {
                *consecutive_no_tool_calls = 0;
            }
            TurnResult::Complete { .. } => {
                *consecutive_no_tool_calls += 1;
            }
            _ => {}
        }

        Ok(turn)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::*;
    use crate::prompts::embedded;
    use crate::subagents::spec::{PermissionAction, PermissionRule};
    use crate::traits::{AgentError, AgentResult, LlmResponse};
    use opendev_runtime::ThinkingLevel;

    fn make_loop() -> ReactLoop {
        ReactLoop::new(ReactLoopConfig {
            max_iterations: Some(10),
            max_nudge_attempts: 3,
            max_todo_nudges: 4,
            ..Default::default()
        })
    }

    #[test]
    fn test_turn_result_equality() {
        assert_eq!(TurnResult::Continue, TurnResult::Continue);
        assert_eq!(TurnResult::Interrupted, TurnResult::Interrupted);
        assert_eq!(TurnResult::MaxIterations, TurnResult::MaxIterations);
        assert_ne!(TurnResult::Continue, TurnResult::Interrupted);
    }

    #[test]
    fn test_process_response_interrupted() {
        let rl = make_loop();
        let resp = LlmResponse::interrupted();
        let result = rl.process_response(&resp, 0);
        assert_eq!(result, TurnResult::Interrupted);
    }

    #[test]
    fn test_process_response_failed() {
        let rl = make_loop();
        let resp = LlmResponse::fail("API error");
        let result = rl.process_response(&resp, 0);
        assert_eq!(result, TurnResult::Continue);
    }

    #[test]
    fn test_process_response_no_tool_calls() {
        let rl = make_loop();
        let msg = serde_json::json!({"role": "assistant", "content": "All done"});
        let resp = LlmResponse::ok(Some("All done".to_string()), msg);
        let result = rl.process_response(&resp, 0);
        match result {
            TurnResult::Complete { content, status } => {
                assert_eq!(content, "All done");
                assert!(status.is_none());
            }
            _ => panic!("Expected Complete"),
        }
    }

    #[test]
    fn test_process_response_with_tool_calls() {
        let rl = make_loop();
        let msg = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "tc-1",
                "function": {"name": "read_file", "arguments": "{}"}
            }]
        });
        let resp = LlmResponse::ok(None, msg);
        let result = rl.process_response(&resp, 0);
        match result {
            TurnResult::ToolCall { tool_calls } => {
                assert_eq!(tool_calls.len(), 1);
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_all_parallelizable_single_tool() {
        let rl = make_loop();
        let tcs = vec![serde_json::json!({
            "function": {"name": "read_file"}
        })];
        // Single tool is not parallelizable (needs > 1)
        assert!(!rl.all_parallelizable(&tcs));
    }

    #[test]
    fn test_all_parallelizable_multiple_read_only() {
        let rl = make_loop();
        let tcs = vec![
            serde_json::json!({"function": {"name": "read_file"}}),
            serde_json::json!({"function": {"name": "search"}}),
        ];
        assert!(rl.all_parallelizable(&tcs));
    }

    #[test]
    fn test_all_parallelizable_with_write_tool() {
        let rl = make_loop();
        let tcs = vec![
            serde_json::json!({"function": {"name": "read_file"}}),
            serde_json::json!({"function": {"name": "write_file"}}),
        ];
        assert!(!rl.all_parallelizable(&tcs));
    }

    #[test]
    fn test_all_parallelizable_with_task_complete() {
        let rl = make_loop();
        let tcs = vec![
            serde_json::json!({"function": {"name": "read_file"}}),
            serde_json::json!({"function": {"name": "task_complete"}}),
        ];
        assert!(!rl.all_parallelizable(&tcs));
    }

    #[test]
    fn test_is_task_complete() {
        let tc = serde_json::json!({
            "function": {"name": "task_complete", "arguments": "{}"}
        });
        assert!(ReactLoop::is_task_complete(&tc));

        let tc2 = serde_json::json!({
            "function": {"name": "read_file", "arguments": "{}"}
        });
        assert!(!ReactLoop::is_task_complete(&tc2));
    }

    #[test]
    fn test_extract_task_complete_args() {
        let tc = serde_json::json!({
            "function": {
                "name": "task_complete",
                "arguments": "{\"result\": \"All done\", \"status\": \"success\"}"
            }
        });
        let (summary, status) = ReactLoop::extract_task_complete_args(&tc);
        assert_eq!(summary, "All done");
        assert_eq!(status, "success");
    }

    #[test]
    fn test_extract_task_complete_args_defaults() {
        let tc = serde_json::json!({
            "function": {"name": "task_complete", "arguments": "{}"}
        });
        let (summary, status) = ReactLoop::extract_task_complete_args(&tc);
        assert_eq!(summary, "Task completed");
        assert_eq!(status, "success");
    }

    #[test]
    fn test_format_tool_result_success() {
        let result = serde_json::json!({"success": true, "output": "file contents"});
        let formatted = ReactLoop::format_tool_result("read_file", &result);
        assert_eq!(formatted, "file contents");
    }

    #[test]
    fn test_format_tool_result_success_with_status() {
        let result = serde_json::json!({
            "success": true,
            "output": "done",
            "completion_status": "partial"
        });
        let formatted = ReactLoop::format_tool_result("write_file", &result);
        assert_eq!(formatted, "[completion_status=partial]\ndone");
    }

    #[test]
    fn test_format_tool_result_failure() {
        let result = serde_json::json!({"success": false, "error": "file not found"});
        let formatted = ReactLoop::format_tool_result("read_file", &result);
        assert_eq!(formatted, "Error in read_file: file not found");
    }

    #[test]
    fn test_classify_error_permission() {
        assert_eq!(
            ReactLoop::classify_error("Permission denied: /etc"),
            "permission_error"
        );
    }

    #[test]
    fn test_classify_error_edit_mismatch() {
        assert_eq!(
            ReactLoop::classify_error("old_content not found in file"),
            "edit_mismatch"
        );
    }

    #[test]
    fn test_classify_error_file_not_found() {
        assert_eq!(
            ReactLoop::classify_error("No such file or directory"),
            "file_not_found"
        );
    }

    #[test]
    fn test_classify_error_syntax() {
        assert_eq!(
            ReactLoop::classify_error("SyntaxError: unexpected token"),
            "syntax_error"
        );
    }

    #[test]
    fn test_classify_error_rate_limit() {
        assert_eq!(
            ReactLoop::classify_error("429 Too Many Requests"),
            "rate_limit"
        );
    }

    #[test]
    fn test_classify_error_timeout() {
        assert_eq!(ReactLoop::classify_error("Request timed out"), "timeout");
    }

    #[test]
    fn test_classify_error_generic() {
        assert_eq!(ReactLoop::classify_error("Something went wrong"), "generic");
    }

    #[test]
    fn test_check_iteration_limit_unlimited() {
        let rl = ReactLoop::new(ReactLoopConfig {
            max_iterations: None,
            ..Default::default()
        });
        assert!(!rl.check_iteration_limit(1));
        assert!(!rl.check_iteration_limit(1000));
    }

    #[test]
    fn test_check_iteration_limit_bounded() {
        let rl = make_loop();
        assert!(!rl.check_iteration_limit(10)); // At limit
        assert!(rl.check_iteration_limit(11)); // Over limit
    }

    #[test]
    fn test_process_iteration_max_iterations() {
        let rl = make_loop();
        let resp = LlmResponse::ok(Some("hello".into()), serde_json::json!({}));
        let mut messages = vec![];
        let mut no_tools = 0;
        let result = rl.process_iteration(&resp, &mut messages, 11, &mut no_tools);
        assert!(matches!(result, Ok(TurnResult::MaxIterations)));
    }

    #[test]
    fn test_process_iteration_interrupted() {
        let rl = make_loop();
        let resp = LlmResponse::interrupted();
        let mut messages = vec![];
        let mut no_tools = 0;
        let result = rl.process_iteration(&resp, &mut messages, 1, &mut no_tools);
        assert!(matches!(result, Ok(TurnResult::Interrupted)));
    }

    #[test]
    fn test_process_iteration_failed() {
        let rl = make_loop();
        let resp = LlmResponse::fail("error");
        let mut messages = vec![];
        let mut no_tools = 0;
        let result = rl.process_iteration(&resp, &mut messages, 1, &mut no_tools);
        assert!(matches!(result, Err(AgentError::LlmError(_))));
    }

    #[test]
    fn test_process_iteration_appends_message() {
        let rl = make_loop();
        let msg = serde_json::json!({"role": "assistant", "content": "hi"});
        let resp = LlmResponse::ok(Some("hi".into()), msg);
        let mut messages = vec![];
        let mut no_tools = 0;
        let _ = rl.process_iteration(&resp, &mut messages, 1, &mut no_tools);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "assistant");
    }

    #[test]
    fn test_process_iteration_increments_no_tool_counter() {
        let rl = make_loop();
        let msg = serde_json::json!({"role": "assistant", "content": "done"});
        let resp = LlmResponse::ok(Some("done".into()), msg);
        let mut messages = vec![];
        let mut no_tools = 0;
        let _ = rl.process_iteration(&resp, &mut messages, 1, &mut no_tools);
        assert_eq!(no_tools, 1);
    }

    #[test]
    fn test_process_iteration_resets_no_tool_counter_on_tool_call() {
        let rl = make_loop();
        let msg = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{"id": "1", "function": {"name": "read_file", "arguments": "{}"}}]
        });
        let resp = LlmResponse::ok(None, msg);
        let mut messages = vec![];
        let mut no_tools = 5;
        let _ = rl.process_iteration(&resp, &mut messages, 1, &mut no_tools);
        assert_eq!(no_tools, 0);
    }

    #[test]
    fn test_default_config() {
        let config = ReactLoopConfig::default();
        assert!(config.max_iterations.is_none());
        assert_eq!(config.max_nudge_attempts, 3);
        assert_eq!(config.max_todo_nudges, 4);
    }

    // --- Thinking skip heuristic tests ---

    #[test]
    fn test_should_skip_thinking_after_readonly() {
        let rl = make_loop();
        let messages = vec![
            serde_json::json!({"role": "user", "content": "read a file"}),
            serde_json::json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{"id": "1", "function": {"name": "read_file", "arguments": "{}"}}]
            }),
            serde_json::json!({"role": "tool", "name": "read_file", "content": "file contents", "tool_call_id": "1"}),
        ];
        assert!(rl.should_skip_thinking(&messages));
    }

    #[test]
    fn test_should_not_skip_thinking_after_write() {
        let rl = make_loop();
        let messages = vec![
            serde_json::json!({"role": "user", "content": "edit a file"}),
            serde_json::json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{"id": "1", "function": {"name": "edit_file", "arguments": "{}"}}]
            }),
            serde_json::json!({"role": "tool", "name": "edit_file", "content": "ok", "tool_call_id": "1"}),
        ];
        assert!(!rl.should_skip_thinking(&messages));
    }

    #[test]
    fn test_should_not_skip_thinking_on_error() {
        let rl = make_loop();
        let messages = vec![
            serde_json::json!({"role": "user", "content": "read"}),
            serde_json::json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{"id": "1", "function": {"name": "read_file", "arguments": "{}"}}]
            }),
            serde_json::json!({"role": "tool", "name": "read_file", "content": "Error: file not found", "tool_call_id": "1"}),
        ];
        assert!(!rl.should_skip_thinking(&messages));
    }

    #[test]
    fn test_should_not_skip_thinking_no_tools() {
        let rl = make_loop();
        let messages = vec![serde_json::json!({"role": "user", "content": "hello"})];
        assert!(!rl.should_skip_thinking(&messages));
    }

    #[test]
    fn test_should_skip_thinking_multiple_readonly() {
        let rl = make_loop();
        let messages = vec![
            serde_json::json!({"role": "user", "content": "search"}),
            serde_json::json!({"role": "assistant", "content": null, "tool_calls": [
                {"id": "1", "function": {"name": "read_file", "arguments": "{}"}},
                {"id": "2", "function": {"name": "search", "arguments": "{}"}}
            ]}),
            serde_json::json!({"role": "tool", "name": "read_file", "content": "ok", "tool_call_id": "1"}),
            serde_json::json!({"role": "tool", "name": "search", "content": "results", "tool_call_id": "2"}),
        ];
        assert!(rl.should_skip_thinking(&messages));
    }

    // --- Shallow subagent detection tests ---

    #[test]
    fn test_shallow_subagent_no_tools() {
        let messages = vec![
            serde_json::json!({"role": "system", "content": "You are..."}),
            serde_json::json!({"role": "user", "content": "do something"}),
            serde_json::json!({"role": "assistant", "content": "Done without tools."}),
        ];
        assert_eq!(ReactLoop::count_subagent_tool_calls(&messages), 0);
        let warning = ReactLoop::shallow_subagent_warning(&messages, true);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("SHALLOW SUBAGENT WARNING"));
    }

    #[test]
    fn test_shallow_subagent_one_tool() {
        let messages = vec![
            serde_json::json!({"role": "assistant", "content": null, "tool_calls": [
                {"id": "1", "function": {"name": "read_file", "arguments": "{}"}}
            ]}),
            serde_json::json!({"role": "tool", "name": "read_file", "content": "ok"}),
            serde_json::json!({"role": "assistant", "content": "Here is the file."}),
        ];
        assert_eq!(ReactLoop::count_subagent_tool_calls(&messages), 1);
        assert!(ReactLoop::shallow_subagent_warning(&messages, true).is_some());
    }

    #[test]
    fn test_not_shallow_subagent_many_tools() {
        let messages = vec![
            serde_json::json!({"role": "assistant", "content": null, "tool_calls": [
                {"id": "1", "function": {"name": "read_file", "arguments": "{}"}}
            ]}),
            serde_json::json!({"role": "tool", "name": "read_file", "content": "ok"}),
            serde_json::json!({"role": "assistant", "content": null, "tool_calls": [
                {"id": "2", "function": {"name": "edit_file", "arguments": "{}"}}
            ]}),
            serde_json::json!({"role": "tool", "name": "edit_file", "content": "ok"}),
            serde_json::json!({"role": "assistant", "content": "Done."}),
        ];
        assert_eq!(ReactLoop::count_subagent_tool_calls(&messages), 2);
        assert!(ReactLoop::shallow_subagent_warning(&messages, true).is_none());
    }

    #[test]
    fn test_shallow_subagent_failed_no_warning() {
        let messages = vec![serde_json::json!({"role": "assistant", "content": "I failed."})];
        assert!(ReactLoop::shallow_subagent_warning(&messages, false).is_none());
    }

    // --- Thinking level configuration tests ---

    #[test]
    fn test_config_thinking_level_default() {
        let config = ReactLoopConfig::default();
        assert_eq!(config.thinking_level, ThinkingLevel::Medium);
        assert!(config.thinking_level.is_enabled());
        assert!(!config.thinking_level.use_critique());
    }

    #[test]
    fn test_config_thinking_level_off_skips_thinking() {
        let config = ReactLoopConfig {
            thinking_level: ThinkingLevel::Off,
            ..Default::default()
        };
        assert!(!config.thinking_level.is_enabled());
    }

    #[test]
    fn test_config_thinking_level_high_enables_critique() {
        let config = ReactLoopConfig {
            thinking_level: ThinkingLevel::High,
            ..Default::default()
        };
        assert!(config.thinking_level.is_enabled());
        assert!(config.thinking_level.use_critique());
    }

    #[test]
    fn test_thinking_skipped_after_readonly_tools() {
        // When last tools were readonly, should_skip_thinking returns true
        // meaning thinking won't run even if level is enabled
        let rl = ReactLoop::new(ReactLoopConfig {
            thinking_level: ThinkingLevel::Medium,
            ..Default::default()
        });
        let messages = vec![
            serde_json::json!({"role": "user", "content": "read something"}),
            serde_json::json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{"id": "1", "function": {"name": "read_file", "arguments": "{}"}}]
            }),
            serde_json::json!({"role": "tool", "name": "read_file", "content": "ok", "tool_call_id": "1"}),
        ];
        assert!(rl.should_skip_thinking(&messages));
    }

    #[test]
    fn test_should_skip_thinking_ignores_thinking_trace() {
        // The thinking trace message (_thinking: true) should be invisible
        // to should_skip_thinking — it should look through it at the real messages.
        let rl = make_loop();
        // Readonly tools followed by thinking trace → still skip
        let messages = vec![
            serde_json::json!({"role": "user", "content": "read something"}),
            serde_json::json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{"id": "1", "function": {"name": "read_file", "arguments": "{}"}}]
            }),
            serde_json::json!({"role": "tool", "name": "read_file", "content": "ok", "tool_call_id": "1"}),
            serde_json::json!({"role": "user", "content": "<thinking_trace>...</thinking_trace>", "_thinking": true}),
        ];
        assert!(rl.should_skip_thinking(&messages));
    }

    #[test]
    fn test_should_not_skip_thinking_with_trace_after_write() {
        // Write tool followed by thinking trace → don't skip
        let rl = make_loop();
        let messages = vec![
            serde_json::json!({"role": "user", "content": "edit something"}),
            serde_json::json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{"id": "1", "function": {"name": "edit_file", "arguments": "{}"}}]
            }),
            serde_json::json!({"role": "tool", "name": "edit_file", "content": "ok", "tool_call_id": "1"}),
            serde_json::json!({"role": "user", "content": "<thinking_trace>...</thinking_trace>", "_thinking": true}),
        ];
        assert!(!rl.should_skip_thinking(&messages));
    }

    #[test]
    fn test_should_not_skip_thinking_only_trace_no_tools() {
        // Only thinking trace, no tool results → don't skip (retryable failure case)
        let rl = make_loop();
        let messages = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({"role": "user", "content": "<thinking_trace>...</thinking_trace>", "_thinking": true}),
        ];
        assert!(!rl.should_skip_thinking(&messages));
    }

    #[test]
    fn test_thinking_not_skipped_after_write_tools() {
        let rl = ReactLoop::new(ReactLoopConfig {
            thinking_level: ThinkingLevel::High,
            ..Default::default()
        });
        let messages = vec![
            serde_json::json!({"role": "user", "content": "edit something"}),
            serde_json::json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{"id": "1", "function": {"name": "edit_file", "arguments": "{}"}}]
            }),
            serde_json::json!({"role": "tool", "name": "edit_file", "content": "ok", "tool_call_id": "1"}),
        ];
        assert!(!rl.should_skip_thinking(&messages));
    }

    #[test]
    fn test_critique_system_prompt_from_template() {
        let critique_prompt = embedded::SYSTEM_CRITIQUE;
        assert!(!critique_prompt.is_empty());
        assert!(
            critique_prompt.to_lowercase().contains("critique")
                || critique_prompt.to_lowercase().contains("critic")
        );
    }

    #[test]
    fn test_config_thinking_system_prompt() {
        let config = ReactLoopConfig {
            thinking_system_prompt: Some("custom thinking prompt".into()),
            original_task: Some("implement feature X".into()),
            ..Default::default()
        };
        assert_eq!(
            config.thinking_system_prompt.as_deref(),
            Some("custom thinking prompt")
        );
        assert_eq!(config.original_task.as_deref(), Some("implement feature X"));
    }

    // --- Iteration metrics tests ---

    #[test]
    fn test_iteration_metrics_default() {
        let metrics = IterationMetrics::default();
        assert_eq!(metrics.iteration, 0);
        assert_eq!(metrics.llm_latency_ms, 0);
        assert_eq!(metrics.input_tokens, 0);
        assert_eq!(metrics.output_tokens, 0);
        assert!(metrics.tool_calls.is_empty());
        assert_eq!(metrics.total_duration_ms, 0);
    }

    #[test]
    fn test_tool_call_metric() {
        let metric = ToolCallMetric {
            tool_name: "read_file".to_string(),
            duration_ms: 42,
            success: true,
        };
        assert_eq!(metric.tool_name, "read_file");
        assert_eq!(metric.duration_ms, 42);
        assert!(metric.success);
    }

    #[test]
    fn test_metrics_accumulation() {
        let rl = make_loop();

        // Initially empty
        assert!(rl.iteration_metrics().is_empty());

        // Push a metric
        rl.push_metrics(IterationMetrics {
            iteration: 1,
            llm_latency_ms: 100,
            input_tokens: 500,
            output_tokens: 200,
            tool_calls: vec![ToolCallMetric {
                tool_name: "read_file".to_string(),
                duration_ms: 10,
                success: true,
            }],
            total_duration_ms: 150,
        });

        let metrics = rl.iteration_metrics();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].iteration, 1);
        assert_eq!(metrics[0].llm_latency_ms, 100);
        assert_eq!(metrics[0].tool_calls.len(), 1);
        assert_eq!(metrics[0].tool_calls[0].tool_name, "read_file");
    }

    #[test]
    fn test_metrics_clear() {
        let rl = make_loop();
        rl.push_metrics(IterationMetrics {
            iteration: 1,
            ..Default::default()
        });
        assert_eq!(rl.iteration_metrics().len(), 1);

        rl.clear_metrics();
        assert!(rl.iteration_metrics().is_empty());
    }

    // --- Partial result preservation tests ---

    #[test]
    fn test_agent_result_interrupted_with_partial_content() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "do stuff"}),
            serde_json::json!({"role": "tool", "name": "read_file", "content": "file data", "tool_call_id": "tc-1"}),
        ];
        let mut result = AgentResult::interrupted(messages);
        // Simulate partial content preservation
        result.content = "Task interrupted by user (partial): I was analyzing...".to_string();
        assert!(result.interrupted);
        assert!(result.content.contains("partial"));
        assert!(result.content.contains("analyzing"));
        // Messages should include the completed tool result
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[1]["name"], "read_file");
    }

    // --- Permission enforcement tests ---

    #[test]
    fn test_evaluate_permission_empty_rules() {
        let config = ReactLoopConfig::default();
        assert!(config.evaluate_permission("run_command", "ls").is_none());
    }

    #[test]
    fn test_evaluate_permission_blanket_deny() {
        let mut config = ReactLoopConfig::default();
        config.permission.insert(
            "run_command".to_string(),
            PermissionRule::Action(PermissionAction::Deny),
        );
        assert_eq!(
            config.evaluate_permission("run_command", "ls"),
            Some(PermissionAction::Deny)
        );
    }

    #[test]
    fn test_evaluate_permission_blanket_allow() {
        let mut config = ReactLoopConfig::default();
        config.permission.insert(
            "read_file".to_string(),
            PermissionRule::Action(PermissionAction::Allow),
        );
        assert_eq!(
            config.evaluate_permission("read_file", ""),
            Some(PermissionAction::Allow)
        );
    }

    #[test]
    fn test_evaluate_permission_wildcard_tool_pattern() {
        let mut config = ReactLoopConfig::default();
        config.permission.insert(
            "*".to_string(),
            PermissionRule::Action(PermissionAction::Ask),
        );
        assert_eq!(
            config.evaluate_permission("write_file", ""),
            Some(PermissionAction::Ask)
        );
    }

    #[test]
    fn test_evaluate_permission_specific_overrides_wildcard() {
        let mut config = ReactLoopConfig::default();
        config.permission.insert(
            "*".to_string(),
            PermissionRule::Action(PermissionAction::Ask),
        );
        config.permission.insert(
            "read_file".to_string(),
            PermissionRule::Action(PermissionAction::Allow),
        );
        // Specific "read_file" should override wildcard "*"
        assert_eq!(
            config.evaluate_permission("read_file", ""),
            Some(PermissionAction::Allow)
        );
        // Wildcard still applies to other tools
        assert_eq!(
            config.evaluate_permission("write_file", ""),
            Some(PermissionAction::Ask)
        );
    }

    #[test]
    fn test_evaluate_permission_pattern_rules() {
        let mut patterns = HashMap::new();
        patterns.insert("*".to_string(), PermissionAction::Ask);
        patterns.insert("git *".to_string(), PermissionAction::Allow);
        patterns.insert("rm -rf *".to_string(), PermissionAction::Deny);

        let mut config = ReactLoopConfig::default();
        config.permission.insert(
            "run_command".to_string(),
            PermissionRule::Patterns(patterns),
        );

        assert_eq!(
            config.evaluate_permission("run_command", "git status"),
            Some(PermissionAction::Allow)
        );
        assert_eq!(
            config.evaluate_permission("run_command", "rm -rf /"),
            Some(PermissionAction::Deny)
        );
        assert_eq!(
            config.evaluate_permission("run_command", "ls -la"),
            Some(PermissionAction::Ask)
        );
    }

    #[test]
    fn test_evaluate_permission_no_match() {
        let mut config = ReactLoopConfig::default();
        config.permission.insert(
            "run_command".to_string(),
            PermissionRule::Action(PermissionAction::Deny),
        );
        // Different tool should not match
        assert!(config.evaluate_permission("read_file", "").is_none());
    }

    #[test]
    fn test_default_config_has_empty_permissions() {
        let config = ReactLoopConfig::default();
        assert!(config.permission.is_empty());
    }

    #[test]
    fn test_mcp_tool_needs_approval_gate() {
        // MCP tools (mcp__*) should be treated the same as run_command
        // for approval purposes
        let tool_name = "mcp__github__create_issue";
        let needs_approval = tool_name == "run_command" || tool_name.starts_with("mcp__");
        assert!(needs_approval);

        // Regular tools should not need the bash approval gate
        let tool_name = "read_file";
        let needs_approval = tool_name == "run_command" || tool_name.starts_with("mcp__");
        assert!(!needs_approval);
    }

    #[test]
    fn test_mcp_permission_rule_matches() {
        let mut config = ReactLoopConfig::default();
        config.permission.insert(
            "mcp__*".to_string(),
            PermissionRule::Action(PermissionAction::Ask),
        );
        // MCP tool should match the glob
        assert_eq!(
            config.evaluate_permission("mcp__sqlite__query", ""),
            Some(PermissionAction::Ask)
        );
        // Non-MCP tool should not match
        assert!(config.evaluate_permission("read_file", "").is_none());
    }

    #[test]
    fn test_mcp_permission_allow_specific() {
        let mut config = ReactLoopConfig::default();
        // Deny all MCP by default
        config.permission.insert(
            "mcp__*".to_string(),
            PermissionRule::Action(PermissionAction::Deny),
        );
        // But allow a specific MCP tool
        config.permission.insert(
            "mcp__sqlite__query".to_string(),
            PermissionRule::Action(PermissionAction::Allow),
        );
        // Specific rule should win (higher specificity)
        assert_eq!(
            config.evaluate_permission("mcp__sqlite__query", ""),
            Some(PermissionAction::Allow)
        );
        // Other MCP tools should be denied
        assert_eq!(
            config.evaluate_permission("mcp__github__push", ""),
            Some(PermissionAction::Deny)
        );
    }
}
