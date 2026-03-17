//! Subagent analysis helpers.

use serde_json::Value;

use super::super::ReactLoop;

impl ReactLoop {
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
}
