use std::collections::HashMap;

use super::mode::AgentMode;
use super::permissions::{PermissionAction, PermissionRule, glob_match, pattern_specificity};
use super::types::SubAgentSpec;

impl SubAgentSpec {
    /// Create a new subagent spec.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        system_prompt: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            system_prompt: system_prompt.into(),
            tools: Vec::new(),
            model: None,
            max_steps: None,
            hidden: false,
            temperature: None,
            top_p: None,
            mode: AgentMode::Subagent,
            max_tokens: None,
            color: None,
            permission: HashMap::new(),
            disable: false,
        }
    }

    /// Set the tools available to this subagent.
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.tools = tools;
        self
    }

    /// Set an override model.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the maximum number of iterations.
    pub fn with_max_steps(mut self, steps: u32) -> Self {
        self.max_steps = Some(steps);
        self
    }

    /// Mark this agent as hidden from UI.
    pub fn with_hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    /// Set an override temperature.
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set an override top_p.
    pub fn with_top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Set the agent mode.
    pub fn with_mode(mut self, mode: AgentMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set an override max_tokens.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Set the display color (hex string like `"#38A3EE"`).
    pub fn with_color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Set the permission rules for this subagent.
    pub fn with_permission(mut self, permission: HashMap<String, PermissionRule>) -> Self {
        self.permission = permission;
        self
    }

    /// Mark this agent as disabled.
    pub fn with_disable(mut self, disable: bool) -> Self {
        self.disable = disable;
        self
    }

    /// Check if this subagent has restricted tools.
    pub fn has_tool_restriction(&self) -> bool {
        !self.tools.is_empty()
    }

    /// Evaluate whether a tool call is permitted by this agent's permission rules.
    ///
    /// Returns the action for the given tool name and argument pattern.
    /// If no matching rule is found, returns `None` (caller decides default).
    ///
    /// More specific patterns take precedence over wildcards.
    /// Within the same specificity level, the last-inserted rule wins.
    pub fn evaluate_permission(
        &self,
        tool_name: &str,
        arg_pattern: &str,
    ) -> Option<PermissionAction> {
        if self.permission.is_empty() {
            return None;
        }

        // Find the most specific matching rule.
        // Specificity: exact match > partial glob > wildcard "*"
        let mut best_match: Option<(PermissionAction, usize)> = None; // (action, specificity)

        for (tool_pattern, rule) in &self.permission {
            if !glob_match(tool_pattern, tool_name) {
                continue;
            }
            match rule {
                PermissionRule::Action(action) => {
                    let specificity = pattern_specificity(tool_pattern);
                    if best_match.as_ref().is_none_or(|(_, s)| specificity >= *s) {
                        best_match = Some((*action, specificity));
                    }
                }
                PermissionRule::Patterns(patterns) => {
                    for (pattern, action) in patterns {
                        if glob_match(pattern, arg_pattern) {
                            let specificity = pattern_specificity(pattern);
                            if best_match.as_ref().is_none_or(|(_, s)| specificity >= *s) {
                                best_match = Some((*action, specificity));
                            }
                        }
                    }
                }
            }
        }

        best_match.map(|(action, _)| action)
    }

    /// Check which tools should be completely disabled (removed from LLM schema).
    ///
    /// A tool is disabled if its last matching rule is a blanket `"deny"` action
    /// (either `PermissionRule::Action(Deny)` or a patterns map with only `"*": "deny"`).
    pub fn disabled_tools(&self, tool_names: &[&str]) -> Vec<String> {
        let mut disabled = Vec::new();
        for &tool in tool_names {
            let is_blanket_deny = self.permission.iter().any(|(tp, rule)| {
                glob_match(tp, tool)
                    && match rule {
                        PermissionRule::Action(PermissionAction::Deny) => true,
                        PermissionRule::Patterns(p) => {
                            p.len() == 1 && p.get("*") == Some(&PermissionAction::Deny)
                        }
                        _ => false,
                    }
            });
            if is_blanket_deny {
                disabled.push(tool.to_string());
            }
        }
        disabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subagent_spec_with_tools() {
        let spec = SubAgentSpec::new("test", "desc", "prompt")
            .with_tools(vec!["read_file".into(), "search".into()]);
        assert!(spec.has_tool_restriction());
        assert_eq!(spec.tools.len(), 2);
    }

    #[test]
    fn test_subagent_spec_with_model() {
        let spec = SubAgentSpec::new("test", "desc", "prompt").with_model("gpt-4");
        assert_eq!(spec.model.as_deref(), Some("gpt-4"));
    }

    #[test]
    fn test_subagent_spec_with_max_steps() {
        let spec = SubAgentSpec::new("test", "desc", "prompt").with_max_steps(50);
        assert_eq!(spec.max_steps, Some(50));
    }

    #[test]
    fn test_subagent_spec_with_hidden() {
        let spec = SubAgentSpec::new("test", "desc", "prompt").with_hidden(true);
        assert!(spec.hidden);
    }

    #[test]
    fn test_subagent_spec_with_temperature() {
        let spec = SubAgentSpec::new("test", "desc", "prompt").with_temperature(0.3);
        assert_eq!(spec.temperature, Some(0.3));
    }

    #[test]
    fn test_with_top_p() {
        let spec = SubAgentSpec::new("test", "desc", "prompt").with_top_p(0.9);
        assert_eq!(spec.top_p, Some(0.9));
    }

    #[test]
    fn test_top_p_serde() {
        let spec = SubAgentSpec::new("test", "desc", "prompt").with_top_p(0.95);
        let json = serde_json::to_string(&spec).unwrap();
        let restored: SubAgentSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.top_p, Some(0.95));
    }

    #[test]
    fn test_with_color() {
        let spec = SubAgentSpec::new("test", "desc", "prompt").with_color("#38A3EE");
        assert_eq!(spec.color.as_deref(), Some("#38A3EE"));
    }

    #[test]
    fn test_color_serde() {
        let spec = SubAgentSpec::new("test", "desc", "prompt").with_color("#FF0000");
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("#FF0000"));
        let restored: SubAgentSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.color.as_deref(), Some("#FF0000"));
    }

    #[test]
    fn test_color_skipped_when_none() {
        let spec = SubAgentSpec::new("test", "desc", "prompt");
        assert!(spec.color.is_none());
        let json = serde_json::to_string(&spec).unwrap();
        assert!(!json.contains("color"));
    }

    #[test]
    fn test_with_max_tokens() {
        let spec = SubAgentSpec::new("test", "desc", "prompt").with_max_tokens(8192);
        assert_eq!(spec.max_tokens, Some(8192));
    }

    #[test]
    fn test_max_tokens_default_none() {
        let spec = SubAgentSpec::new("test", "desc", "prompt");
        assert!(spec.max_tokens.is_none());
    }

    #[test]
    fn test_max_tokens_serde() {
        let spec = SubAgentSpec::new("test", "desc", "prompt").with_max_tokens(16384);
        let json = serde_json::to_string(&spec).unwrap();
        let restored: SubAgentSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.max_tokens, Some(16384));
    }

    #[test]
    fn test_evaluate_permission_blanket_action() {
        let mut perms = HashMap::new();
        perms.insert(
            "bash".to_string(),
            PermissionRule::Action(PermissionAction::Deny),
        );

        let spec = SubAgentSpec::new("test", "desc", "prompt").with_permission(perms);

        assert_eq!(
            spec.evaluate_permission("bash", "anything"),
            Some(PermissionAction::Deny)
        );
        assert_eq!(
            spec.evaluate_permission("read_file", "anything"),
            None // No rule for read_file
        );
    }

    #[test]
    fn test_evaluate_permission_wildcard_tool() {
        let mut perms = HashMap::new();
        perms.insert(
            "*".to_string(),
            PermissionRule::Action(PermissionAction::Ask),
        );

        let spec = SubAgentSpec::new("test", "desc", "prompt").with_permission(perms);

        assert_eq!(
            spec.evaluate_permission("bash", "anything"),
            Some(PermissionAction::Ask)
        );
        assert_eq!(
            spec.evaluate_permission("read_file", "anything"),
            Some(PermissionAction::Ask)
        );
    }

    #[test]
    fn test_evaluate_permission_pattern_matching() {
        let mut patterns = HashMap::new();
        patterns.insert("*".to_string(), PermissionAction::Ask);
        patterns.insert("git *".to_string(), PermissionAction::Allow);
        patterns.insert("rm -rf *".to_string(), PermissionAction::Deny);

        let mut perms = HashMap::new();
        perms.insert("bash".to_string(), PermissionRule::Patterns(patterns));

        let spec = SubAgentSpec::new("test", "desc", "prompt").with_permission(perms);

        assert_eq!(
            spec.evaluate_permission("bash", "git status"),
            Some(PermissionAction::Allow)
        );
        assert_eq!(
            spec.evaluate_permission("bash", "rm -rf /"),
            Some(PermissionAction::Deny)
        );
        assert_eq!(
            spec.evaluate_permission("bash", "npm install"),
            Some(PermissionAction::Ask)
        );
    }

    #[test]
    fn test_evaluate_permission_no_rules() {
        let spec = SubAgentSpec::new("test", "desc", "prompt");
        assert_eq!(spec.evaluate_permission("bash", "anything"), None);
    }

    #[test]
    fn test_disabled_tools_blanket_deny() {
        let mut perms = HashMap::new();
        perms.insert(
            "edit".to_string(),
            PermissionRule::Action(PermissionAction::Deny),
        );
        perms.insert(
            "bash".to_string(),
            PermissionRule::Action(PermissionAction::Allow),
        );

        let spec = SubAgentSpec::new("test", "desc", "prompt").with_permission(perms);

        let disabled = spec.disabled_tools(&["edit", "bash", "read_file"]);
        assert_eq!(disabled, vec!["edit"]);
    }

    #[test]
    fn test_disabled_tools_pattern_deny_not_blanket() {
        // Pattern-specific deny should NOT disable the tool entirely.
        let mut patterns = HashMap::new();
        patterns.insert("rm *".to_string(), PermissionAction::Deny);
        patterns.insert("*".to_string(), PermissionAction::Allow);

        let mut perms = HashMap::new();
        perms.insert("bash".to_string(), PermissionRule::Patterns(patterns));

        let spec = SubAgentSpec::new("test", "desc", "prompt").with_permission(perms);

        let disabled = spec.disabled_tools(&["bash"]);
        assert!(
            disabled.is_empty(),
            "Pattern-specific deny should not disable tool"
        );
    }

    #[test]
    fn test_disable_field() {
        let spec = SubAgentSpec::new("test", "desc", "prompt").with_disable(true);
        assert!(spec.disable);

        let spec2 = SubAgentSpec::new("test", "desc", "prompt");
        assert!(!spec2.disable);
    }

    #[test]
    fn test_disable_serde() {
        let spec = SubAgentSpec::new("test", "desc", "prompt").with_disable(true);
        let json = serde_json::to_string(&spec).unwrap();
        let restored: SubAgentSpec = serde_json::from_str(&json).unwrap();
        assert!(restored.disable);
    }
}
