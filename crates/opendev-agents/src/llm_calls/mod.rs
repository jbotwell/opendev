//! LLM API call methods.
//!
//! Mirrors `opendev/core/agents/main_agent/llm_calls.py`.
//! Provides `LlmCaller` with methods for normal, thinking, critique, and compact calls.

mod model_detection;

use model_detection::{insert_max_tokens, insert_temperature};
pub use model_detection::{is_reasoning_model, supports_temperature, uses_max_completion_tokens};

use serde_json::Value;
use tracing::{debug, warn};

use crate::response::ResponseCleaner;
use crate::traits::LlmResponse;

/// Configuration for an LLM call.
#[derive(Debug, Clone)]
pub struct LlmCallConfig {
    /// Model identifier (e.g. "gpt-4o", "claude-3-opus").
    pub model: String,
    /// Temperature for sampling.
    pub temperature: Option<f64>,
    /// Maximum tokens to generate.
    pub max_tokens: Option<u64>,
    /// Reasoning effort level ("low", "medium", "high", or "none").
    pub reasoning_effort: Option<String>,
}

/// Handles different types of LLM calls (normal, compact).
///
/// Uses composition instead of Python's mixin pattern. Holds a `ResponseCleaner`
/// and call configuration, producing structured `LlmResponse` values.
#[derive(Debug, Clone)]
pub struct LlmCaller {
    cleaner: ResponseCleaner,
    /// Primary model config.
    pub config: LlmCallConfig,
}

impl LlmCaller {
    /// Create a new LLM caller with the given primary model configuration.
    pub fn new(config: LlmCallConfig) -> Self {
        Self {
            cleaner: ResponseCleaner::new(),
            config,
        }
    }

    /// Strip internal `_`-prefixed keys and filter out `Internal`-class messages
    /// before API calls.
    pub fn clean_messages(messages: &[Value]) -> Vec<Value> {
        messages
            .iter()
            .filter(|msg| msg.get("_msg_class").and_then(|v| v.as_str()) != Some("internal"))
            .map(|msg| {
                if let Some(obj) = msg.as_object() {
                    if obj.keys().any(|k| k.starts_with('_')) {
                        let cleaned: serde_json::Map<String, Value> = obj
                            .iter()
                            .filter(|(k, _)| !k.starts_with('_'))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        Value::Object(cleaned)
                    } else {
                        msg.clone()
                    }
                } else {
                    msg.clone()
                }
            })
            .collect()
    }

    /// Build an LLM payload for an action call (with tools).
    pub fn build_action_payload(&self, messages: &[Value], tool_schemas: &[Value]) -> Value {
        let mut payload = serde_json::json!({
            "model": self.config.model,
            "messages": Self::clean_messages(messages),
            "tools": tool_schemas,
            "tool_choice": "auto",
        });

        if let Some(temp) = self.config.temperature {
            insert_temperature(&mut payload, &self.config.model, temp);
        }
        if let Some(max) = self.config.max_tokens {
            insert_max_tokens(&mut payload, &self.config.model, max);
        }

        // Inject reasoning effort for adapters to consume
        if let Some(ref effort) = self.config.reasoning_effort {
            payload["_reasoning_effort"] = serde_json::json!(effort);
        }

        payload
    }

    /// Parse an action response (with potential tool calls) into an `LlmResponse`.
    pub fn parse_action_response(&self, body: &Value) -> LlmResponse {
        let choices = match body.get("choices").and_then(|c| c.as_array()) {
            Some(c) if !c.is_empty() => c,
            _ => {
                warn!("No choices in LLM response");
                return LlmResponse::fail("No choices in response");
            }
        };

        let choice = &choices[0];
        let message = match choice.get("message") {
            Some(m) => m,
            None => {
                warn!("No message in choice");
                return LlmResponse::fail("No message in response choice");
            }
        };

        let raw_content = message.get("content").and_then(|c| c.as_str());
        let cleaned_content = self.cleaner.clean(raw_content);
        let reasoning_content = message
            .get("reasoning_content")
            .and_then(|r| r.as_str())
            .map(|s| s.to_string());

        debug!(
            has_content = raw_content.is_some(),
            has_tool_calls = message.get("tool_calls").is_some(),
            "Parsed action response"
        );

        let mut resp = LlmResponse::ok(cleaned_content, message.clone());
        resp.usage = body.get("usage").cloned();
        resp.reasoning_content = reasoning_content;
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_caller() -> LlmCaller {
        LlmCaller::new(LlmCallConfig {
            model: "gpt-4o".to_string(),
            temperature: Some(0.7),
            max_tokens: Some(4096),
            reasoning_effort: None,
        })
    }

    #[test]
    fn test_clean_messages_strips_underscore_keys() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "hello", "_internal": true}),
            serde_json::json!({"role": "assistant", "content": "world"}),
        ];
        let cleaned = LlmCaller::clean_messages(&messages);
        assert!(cleaned[0].get("_internal").is_none());
        assert_eq!(cleaned[0]["role"], "user");
        assert_eq!(cleaned[1]["role"], "assistant");
    }

    #[test]
    fn test_clean_messages_preserves_non_object() {
        let messages = vec![serde_json::json!("string_value")];
        let cleaned = LlmCaller::clean_messages(&messages);
        assert_eq!(cleaned[0], "string_value");
    }

    #[test]
    fn test_clean_messages_strips_internal() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({"role": "user", "content": "[SYSTEM] debug", "_msg_class": "internal"}),
            serde_json::json!({"role": "user", "content": "[SYSTEM] error", "_msg_class": "directive"}),
            serde_json::json!({"role": "user", "content": "[SYSTEM] nudge", "_msg_class": "nudge"}),
        ];
        let cleaned = LlmCaller::clean_messages(&messages);
        assert_eq!(cleaned.len(), 3);
        assert_eq!(cleaned[0]["content"], "hello");
        assert_eq!(cleaned[1]["content"], "[SYSTEM] error");
        assert_eq!(cleaned[2]["content"], "[SYSTEM] nudge");
        assert!(cleaned[1].get("_msg_class").is_none());
        assert!(cleaned[2].get("_msg_class").is_none());
    }

    #[test]
    fn test_build_action_payload() {
        let caller = make_caller();
        let messages = vec![serde_json::json!({"role": "user", "content": "do something"})];
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {"name": "read_file", "parameters": {}}
        })];
        let payload = caller.build_action_payload(&messages, &tools);
        assert_eq!(payload["model"], "gpt-4o");
        assert_eq!(payload["tool_choice"], "auto");
        assert!(payload["tools"].as_array().unwrap().len() == 1);
        assert_eq!(payload["temperature"], 0.7);
    }

    #[test]
    fn test_parse_action_response_success() {
        let caller = make_caller();
        let body = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "Hello world", "tool_calls": null}}],
            "usage": {"total_tokens": 100}
        });
        let resp = caller.parse_action_response(&body);
        assert!(resp.success);
        assert_eq!(resp.content.as_deref(), Some("Hello world"));
        assert!(resp.usage.is_some());
    }

    #[test]
    fn test_parse_action_response_with_tool_calls() {
        let caller = make_caller();
        let body = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": null,
                "tool_calls": [{"id": "tc-1", "function": {"name": "read_file", "arguments": "{\"path\": \"test.rs\"}"}}]
            }}]
        });
        let resp = caller.parse_action_response(&body);
        assert!(resp.success);
        assert!(resp.content.is_none());
        assert!(resp.tool_calls.is_some());
        assert_eq!(resp.tool_calls.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_parse_action_response_no_choices() {
        let caller = make_caller();
        let body = serde_json::json!({"choices": []});
        let resp = caller.parse_action_response(&body);
        assert!(!resp.success);
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_parse_response_cleans_provider_tokens() {
        let caller = make_caller();
        let body = serde_json::json!({"choices": [{"message": {"role": "assistant", "content": "Hello<|im_end|> world"}}]});
        let resp = caller.parse_action_response(&body);
        assert!(resp.success);
        assert_eq!(resp.content.as_deref(), Some("Hello world"));
    }

    #[test]
    fn test_action_payload_reasoning_model() {
        let caller = LlmCaller::new(LlmCallConfig {
            model: "o3-mini".to_string(),
            temperature: Some(0.7),
            max_tokens: Some(4096),
            reasoning_effort: None,
        });
        let messages = vec![serde_json::json!({"role": "user", "content": "test"})];
        let tools = vec![serde_json::json!({"type": "function", "function": {"name": "test"}})];
        let payload = caller.build_action_payload(&messages, &tools);
        assert_eq!(payload["max_completion_tokens"], 4096);
        assert!(payload.get("max_tokens").is_none());
        assert!(payload.get("temperature").is_none());
    }

    #[test]
    fn test_parse_response_with_reasoning_content() {
        let caller = make_caller();
        let body = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "The answer is 42.", "reasoning_content": "Let me think step by step..."}}]
        });
        let resp = caller.parse_action_response(&body);
        assert!(resp.success);
        assert_eq!(
            resp.reasoning_content.as_deref(),
            Some("Let me think step by step...")
        );
    }
}
