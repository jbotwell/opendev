use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::mode::AgentMode;
use super::permissions::PermissionRule;

/// Specification for defining a subagent.
///
/// Subagents are ephemeral agents that handle isolated tasks.
/// They receive a task description, execute with their own context,
/// and return a single result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentSpec {
    /// Unique identifier for the subagent type.
    pub name: String,

    /// Human-readable description of what this subagent does.
    pub description: String,

    /// System prompt that defines the subagent's behavior and role.
    pub system_prompt: String,

    /// List of tool names this subagent has access to.
    /// If empty, inherits all tools from the main agent.
    #[serde(default)]
    pub tools: Vec<String>,

    /// Override model for this subagent.
    /// If None, uses the same model as the main agent.
    #[serde(default)]
    pub model: Option<String>,

    /// Maximum number of ReAct loop iterations for this subagent.
    /// If None, uses the default limit (25).
    #[serde(default)]
    pub max_steps: Option<u32>,

    /// Whether this agent is hidden from UI/menu selection.
    /// Hidden agents (like internal compaction agents) are not shown
    /// in the agent list but can still be spawned programmatically.
    #[serde(default)]
    pub hidden: bool,

    /// Override temperature for this subagent.
    /// If None, uses the default (0.7).
    #[serde(default)]
    pub temperature: Option<f32>,

    /// Override top_p (nucleus sampling) for this subagent.
    /// If None, uses the provider default.
    #[serde(default)]
    pub top_p: Option<f32>,

    /// Agent mode classification.
    /// - `primary`: Main agents that handle top-level conversations.
    /// - `subagent`: Can only be spawned via spawn_subagent tool.
    /// - `all`: Can function in both primary and subagent roles.
    #[serde(default = "AgentMode::default_mode")]
    pub mode: AgentMode,

    /// Override max_tokens for this subagent's LLM calls.
    /// If None, inherits parent agent's max_tokens from model registry.
    #[serde(default)]
    pub max_tokens: Option<u32>,

    /// Optional hex color for TUI display (e.g., `"#38A3EE"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    /// Per-tool permission rules.
    ///
    /// Maps tool names to permission rules. Each rule can be:
    /// - A single action string (`"allow"`, `"deny"`, `"ask"`)
    /// - A map of glob patterns to actions (`{ "git *": "allow", "rm *": "deny" }`)
    ///
    /// Tool names support wildcards (`"*"` = all tools, `"read_*"` = all read tools).
    /// Last matching rule wins when multiple patterns match.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub permission: HashMap<String, PermissionRule>,

    /// Whether this agent is disabled (not available for use).
    #[serde(default)]
    pub disable: bool,
}

#[cfg(test)]
mod tests {
    use super::super::permissions::PermissionAction;
    use super::*;

    #[test]
    fn test_subagent_spec_new() {
        let spec = SubAgentSpec::new("test", "A test agent", "You are a test agent.");
        assert_eq!(spec.name, "test");
        assert!(!spec.has_tool_restriction());
        assert!(spec.model.is_none());
    }

    #[test]
    fn test_subagent_spec_serde() {
        let spec = SubAgentSpec::new("test", "desc", "prompt")
            .with_tools(vec!["read_file".into()])
            .with_model("gpt-4");

        let json = serde_json::to_string(&spec).unwrap();
        let restored: SubAgentSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "test");
        assert_eq!(restored.tools, vec!["read_file"]);
        assert_eq!(restored.model.as_deref(), Some("gpt-4"));
    }

    #[test]
    fn test_subagent_spec_defaults() {
        let spec = SubAgentSpec::new("test", "desc", "prompt");
        assert!(spec.max_steps.is_none());
        assert!(!spec.hidden);
        assert!(spec.temperature.is_none());
        assert!(spec.top_p.is_none());
        assert_eq!(spec.mode, AgentMode::Subagent);
    }

    #[test]
    fn test_subagent_spec_serde_extended_fields() {
        let spec = SubAgentSpec::new("test", "desc", "prompt")
            .with_max_steps(50)
            .with_hidden(true)
            .with_temperature(0.5);

        let json = serde_json::to_string(&spec).unwrap();
        let restored: SubAgentSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.max_steps, Some(50));
        assert!(restored.hidden);
        assert_eq!(restored.temperature, Some(0.5));
    }
}
