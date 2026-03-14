//! Standardized message conversion between ChatMessage (persistence) and
//! OpenAI-style API Values (LLM context).
//!
//! This is the single source of truth for both CLI and Web UI.

use std::collections::HashMap;

use chrono::Utc;
use serde_json::{Value, json};
use tracing::warn;

use opendev_models::message::{ChatMessage, Role, ToolCall};
use opendev_models::validator::validate_message;

/// Convert stored `ChatMessage`s to OpenAI-compatible API values.
///
/// - User/System → `{"role": "...", "content": "..."}`
/// - Assistant without tools → `{"role": "assistant", "content": "..."}`
/// - Assistant with tools → assistant message with `tool_calls` array,
///   followed by one `{"role": "tool", ...}` message per tool call result.
pub fn chatmessages_to_api_values(messages: &[ChatMessage]) -> Vec<Value> {
    let mut result = Vec::new();

    for msg in messages {
        match msg.role {
            Role::User => {
                // Skip internal thinking markers
                if msg.metadata.contains_key("_thinking") {
                    continue;
                }
                result.push(json!({
                    "role": "user",
                    "content": &msg.content,
                }));
            }
            Role::System => {
                result.push(json!({
                    "role": "system",
                    "content": &msg.content,
                }));
            }
            Role::Assistant => {
                if msg.tool_calls.is_empty() {
                    // Simple text response
                    let mut val = json!({
                        "role": "assistant",
                        "content": &msg.content,
                    });
                    // Attach thinking trace if present
                    if let Some(ref trace) = msg.thinking_trace {
                        val["_thinking_trace"] = json!(trace);
                    }
                    if let Some(ref reasoning) = msg.reasoning_content {
                        val["reasoning_content"] = json!(reasoning);
                    }
                    result.push(val);
                } else {
                    // Assistant with tool calls
                    let tool_calls_api: Vec<Value> = msg
                        .tool_calls
                        .iter()
                        .map(|tc| {
                            let args_str = serde_json::to_string(&tc.parameters)
                                .unwrap_or_else(|_| "{}".to_string());
                            json!({
                                "id": &tc.id,
                                "type": "function",
                                "function": {
                                    "name": &tc.name,
                                    "arguments": args_str,
                                }
                            })
                        })
                        .collect();

                    let content: Value = if msg.content.is_empty() {
                        Value::Null
                    } else {
                        Value::String(msg.content.clone())
                    };

                    let mut assistant_val = json!({
                        "role": "assistant",
                        "content": content,
                        "tool_calls": tool_calls_api,
                    });
                    if let Some(ref trace) = msg.thinking_trace {
                        assistant_val["_thinking_trace"] = json!(trace);
                    }
                    if let Some(ref reasoning) = msg.reasoning_content {
                        assistant_val["reasoning_content"] = json!(reasoning);
                    }
                    result.push(assistant_val);

                    // Emit tool result messages
                    for tc in &msg.tool_calls {
                        let tool_content = if let Some(ref err) = tc.error {
                            format!("Error: {err}")
                        } else if let Some(ref res) = tc.result {
                            match res {
                                Value::String(s) => s.clone(),
                                other => serde_json::to_string(other)
                                    .unwrap_or_else(|_| "null".to_string()),
                            }
                        } else {
                            "No result available.".to_string()
                        };

                        result.push(json!({
                            "role": "tool",
                            "tool_call_id": &tc.id,
                            "name": &tc.name,
                            "content": tool_content,
                        }));
                    }
                }
            }
        }
    }

    result
}

/// Convert OpenAI-style API values back to `ChatMessage`s for persistence.
///
/// Groups `role: "tool"` messages with their preceding `role: "assistant"` message
/// by matching `tool_call_id`.
pub fn api_values_to_chatmessages(values: &[Value]) -> Vec<ChatMessage> {
    let mut result = Vec::new();
    let mut i = 0;

    while i < values.len() {
        let val = &values[i];
        let role_str = val["role"].as_str().unwrap_or("");

        match role_str {
            "user" => {
                // Skip thinking markers
                if val
                    .get("_thinking")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    // Attach thinking trace to the last assistant message if possible
                    if let Some(content) = val["content"].as_str()
                        && let Some(last) = result.last_mut()
                    {
                        let last_msg: &mut ChatMessage = last;
                        if last_msg.role == Role::Assistant && last_msg.thinking_trace.is_none() {
                            last_msg.thinking_trace = Some(content.to_string());
                        }
                    }
                    i += 1;
                    continue;
                }

                result.push(ChatMessage {
                    role: Role::User,
                    content: val["content"].as_str().unwrap_or("").to_string(),
                    timestamp: Utc::now(),
                    metadata: HashMap::new(),
                    tool_calls: Vec::new(),
                    tokens: None,
                    thinking_trace: None,
                    reasoning_content: None,
                    token_usage: None,
                    provenance: None,
                });
            }
            "system" => {
                result.push(ChatMessage {
                    role: Role::System,
                    content: val["content"].as_str().unwrap_or("").to_string(),
                    timestamp: Utc::now(),
                    metadata: HashMap::new(),
                    tool_calls: Vec::new(),
                    tokens: None,
                    thinking_trace: None,
                    reasoning_content: None,
                    token_usage: None,
                    provenance: None,
                });
            }
            "assistant" => {
                let content = match &val["content"] {
                    Value::String(s) => s.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                };

                let thinking_trace = val
                    .get("_thinking_trace")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let reasoning_content = val
                    .get("reasoning_content")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                // Parse tool_calls array
                let mut tool_calls = Vec::new();
                if let Some(tcs) = val.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc_val in tcs {
                        let id = tc_val["id"].as_str().unwrap_or("").to_string();
                        let name = tc_val["function"]["name"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        let args_str = tc_val["function"]["arguments"].as_str().unwrap_or("{}");
                        let parameters: HashMap<String, Value> =
                            serde_json::from_str(args_str).unwrap_or_default();

                        tool_calls.push(ToolCall {
                            id,
                            name,
                            parameters,
                            result: None,
                            result_summary: None,
                            timestamp: Utc::now(),
                            approved: true,
                            error: None,
                            nested_tool_calls: Vec::new(),
                        });
                    }
                }

                // Consume subsequent tool result messages
                let mut j = i + 1;
                while j < values.len() {
                    let next = &values[j];
                    if next["role"].as_str() != Some("tool") {
                        break;
                    }
                    let tool_call_id = next["tool_call_id"].as_str().unwrap_or("");
                    let tool_content = next["content"].as_str().unwrap_or("").to_string();

                    // Match to tool call by id
                    if let Some(tc) = tool_calls.iter_mut().find(|tc| tc.id == tool_call_id) {
                        if tool_content.starts_with("Error: ") {
                            tc.error = Some(tool_content.trim_start_matches("Error: ").to_string());
                        } else {
                            tc.result = Some(Value::String(tool_content));
                        }
                    }
                    j += 1;
                }

                // Repair tool calls without results
                for tc in &mut tool_calls {
                    if tc.result.is_none() && tc.error.is_none() && tc.name != "task_complete" {
                        tc.error =
                            Some("Tool execution was interrupted or never completed.".to_string());
                    }
                }

                result.push(ChatMessage {
                    role: Role::Assistant,
                    content,
                    timestamp: Utc::now(),
                    metadata: HashMap::new(),
                    tool_calls,
                    tokens: None,
                    thinking_trace,
                    reasoning_content,
                    token_usage: None,
                    provenance: None,
                });

                // Skip the consumed tool messages
                i = j;
                continue;
            }
            "tool" => {
                // Standalone tool message not consumed by an assistant —
                // this shouldn't normally happen but handle gracefully
                warn!(
                    "Orphaned tool message for tool_call_id={}",
                    val["tool_call_id"].as_str().unwrap_or("?")
                );
            }
            _ => {
                warn!("Unknown message role: {}", role_str);
            }
        }

        i += 1;
    }

    // Validate each message
    result.retain(|msg| {
        let verdict = validate_message(msg);
        if !verdict.is_valid {
            warn!("Dropping invalid converted message: {}", verdict.reason);
            false
        } else {
            true
        }
    });

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_user(content: &str) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: content.to_string(),
            timestamp: Utc::now(),
            metadata: HashMap::new(),
            tool_calls: Vec::new(),
            tokens: None,
            thinking_trace: None,
            reasoning_content: None,
            token_usage: None,
            provenance: None,
        }
    }

    fn make_assistant(content: &str) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            content: content.to_string(),
            timestamp: Utc::now(),
            metadata: HashMap::new(),
            tool_calls: Vec::new(),
            tokens: None,
            thinking_trace: None,
            reasoning_content: None,
            token_usage: None,
            provenance: None,
        }
    }

    fn make_assistant_with_tools(content: &str, tool_calls: Vec<ToolCall>) -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            content: content.to_string(),
            timestamp: Utc::now(),
            metadata: HashMap::new(),
            tool_calls,
            tokens: None,
            thinking_trace: None,
            reasoning_content: None,
            token_usage: None,
            provenance: None,
        }
    }

    fn make_tool_call(id: &str, name: &str, result: Option<&str>, error: Option<&str>) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            parameters: HashMap::new(),
            result: result.map(|s| Value::String(s.to_string())),
            result_summary: None,
            timestamp: Utc::now(),
            approved: true,
            error: error.map(String::from),
            nested_tool_calls: Vec::new(),
        }
    }

    #[test]
    fn test_simple_roundtrip() {
        let messages = vec![make_user("Hello"), make_assistant("Hi there!")];

        let api_values = chatmessages_to_api_values(&messages);
        assert_eq!(api_values.len(), 2);
        assert_eq!(api_values[0]["role"], "user");
        assert_eq!(api_values[1]["role"], "assistant");

        let restored = api_values_to_chatmessages(&api_values);
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].role, Role::User);
        assert_eq!(restored[0].content, "Hello");
        assert_eq!(restored[1].role, Role::Assistant);
        assert_eq!(restored[1].content, "Hi there!");
    }

    #[test]
    fn test_tool_calls_roundtrip() {
        let tc = make_tool_call("tc-1", "bash", Some("output here"), None);
        let messages = vec![
            make_user("Run ls"),
            make_assistant_with_tools("Let me run that.", vec![tc]),
        ];

        let api_values = chatmessages_to_api_values(&messages);
        // user + assistant + tool result = 3
        assert_eq!(api_values.len(), 3);
        assert_eq!(api_values[1]["role"], "assistant");
        assert!(api_values[1]["tool_calls"].is_array());
        assert_eq!(api_values[2]["role"], "tool");
        assert_eq!(api_values[2]["tool_call_id"], "tc-1");

        let restored = api_values_to_chatmessages(&api_values);
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[1].tool_calls.len(), 1);
        assert_eq!(restored[1].tool_calls[0].name, "bash");
        assert!(restored[1].tool_calls[0].result.is_some());
    }

    #[test]
    fn test_tool_call_error_roundtrip() {
        let tc = make_tool_call("tc-2", "bash", None, Some("command not found"));
        let messages = vec![make_assistant_with_tools("Running command", vec![tc])];

        let api_values = chatmessages_to_api_values(&messages);
        assert_eq!(api_values.len(), 2);
        assert_eq!(api_values[1]["content"], "Error: command not found");

        let restored = api_values_to_chatmessages(&api_values);
        assert_eq!(restored.len(), 1);
        assert!(restored[0].tool_calls[0].error.is_some());
        assert_eq!(
            restored[0].tool_calls[0].error.as_deref(),
            Some("command not found")
        );
    }

    #[test]
    fn test_empty_content_with_tool_calls() {
        let tc = make_tool_call("tc-3", "read_file", Some("file contents"), None);
        let messages = vec![make_assistant_with_tools("", vec![tc])];

        let api_values = chatmessages_to_api_values(&messages);
        assert!(api_values[0]["content"].is_null());

        let restored = api_values_to_chatmessages(&api_values);
        assert_eq!(restored.len(), 1);
        assert!(restored[0].content.is_empty());
        assert_eq!(restored[0].tool_calls.len(), 1);
    }

    #[test]
    fn test_thinking_trace_preserved() {
        let mut msg = make_assistant("Got it.");
        msg.thinking_trace = Some("I should check the file first.".to_string());
        let messages = vec![make_user("Do this"), msg];

        let api_values = chatmessages_to_api_values(&messages);
        assert_eq!(
            api_values[1]["_thinking_trace"],
            "I should check the file first."
        );

        let restored = api_values_to_chatmessages(&api_values);
        assert_eq!(
            restored[1].thinking_trace.as_deref(),
            Some("I should check the file first.")
        );
    }

    #[test]
    fn test_incomplete_tool_call_gets_synthetic_error() {
        // Simulate an assistant message with tool_calls but no subsequent tool result
        let api_values = vec![
            json!({"role": "user", "content": "Run ls"}),
            json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "tc-orphan",
                    "type": "function",
                    "function": {
                        "name": "bash",
                        "arguments": "{}"
                    }
                }]
            }),
            // No tool result message follows
        ];

        let restored = api_values_to_chatmessages(&api_values);
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[1].tool_calls.len(), 1);
        assert!(restored[1].tool_calls[0].error.is_some());
        assert!(
            restored[1].tool_calls[0]
                .error
                .as_deref()
                .unwrap()
                .contains("interrupted")
        );
    }

    #[test]
    fn test_thinking_marker_skipped() {
        let api_values = vec![
            json!({"role": "user", "content": "Hello"}),
            json!({"role": "assistant", "content": "Hi"}),
            json!({"role": "user", "content": "Think about this", "_thinking": true}),
            json!({"role": "user", "content": "Next question"}),
        ];

        let restored = api_values_to_chatmessages(&api_values);
        // The _thinking user message should be skipped, leaving 3 messages
        assert_eq!(restored.len(), 3);
        assert_eq!(restored[0].content, "Hello");
        assert_eq!(restored[1].content, "Hi");
        assert_eq!(restored[2].content, "Next question");
    }

    #[test]
    fn test_multiple_tool_calls() {
        let tc1 = make_tool_call("tc-a", "bash", Some("result 1"), None);
        let tc2 = make_tool_call("tc-b", "read_file", Some("result 2"), None);
        let messages = vec![make_assistant_with_tools(
            "Running multiple tools",
            vec![tc1, tc2],
        )];

        let api_values = chatmessages_to_api_values(&messages);
        // assistant + 2 tool results = 3
        assert_eq!(api_values.len(), 3);

        let restored = api_values_to_chatmessages(&api_values);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].tool_calls.len(), 2);
    }

    #[test]
    fn test_system_message_roundtrip() {
        let messages = vec![ChatMessage {
            role: Role::System,
            content: "You are a helpful assistant.".to_string(),
            timestamp: Utc::now(),
            metadata: HashMap::new(),
            tool_calls: Vec::new(),
            tokens: None,
            thinking_trace: None,
            reasoning_content: None,
            token_usage: None,
            provenance: None,
        }];

        let api_values = chatmessages_to_api_values(&messages);
        assert_eq!(api_values[0]["role"], "system");

        let restored = api_values_to_chatmessages(&api_values);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].role, Role::System);
    }
}
