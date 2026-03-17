//! ReactLoopConfig: configuration and permission evaluation.

use std::collections::HashMap;

use crate::agent_types::AgentDefinition;
use crate::subagents::spec::{PermissionAction, PermissionRule};
use opendev_runtime::ThinkingLevel;

/// Configuration for the ReAct loop.
#[derive(Debug, Clone)]
pub struct ReactLoopConfig {
    /// Maximum number of iterations (None = unlimited).
    pub max_iterations: Option<usize>,
    /// Maximum consecutive no-tool-call responses before accepting completion.
    pub max_nudge_attempts: usize,
    /// Maximum todo completion nudges before allowing completion anyway.
    pub max_todo_nudges: usize,
    /// Thinking level — controls whether thinking/critique phases run.
    pub thinking_level: ThinkingLevel,
    /// Pre-composed thinking system prompt (from `create_thinking_composer`).
    /// If `None`, the thinking phase will not swap the system prompt.
    pub thinking_system_prompt: Option<String>,
    /// The user's original task text, used for analysis prompt construction.
    pub original_task: Option<String>,
    /// Optional agent definition — when set, the loop uses the agent's
    /// thinking/critique model overrides and thinking level.
    pub agent_definition: Option<AgentDefinition>,
    /// Per-agent permission rules for tool access control.
    /// Maps tool name patterns to permission rules (allow/deny/ask).
    /// When non-empty, each tool call is checked against these rules
    /// before execution.
    pub permission: HashMap<String, PermissionRule>,
}

impl Default for ReactLoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: None, // Unlimited by default (matches Python)
            max_nudge_attempts: 3,
            max_todo_nudges: 4,
            thinking_level: ThinkingLevel::Medium,
            thinking_system_prompt: None,
            original_task: None,
            agent_definition: None,
            permission: HashMap::new(),
        }
    }
}

impl ReactLoopConfig {
    /// Return the effective thinking level, considering the agent definition override.
    pub fn effective_thinking_level(&self) -> ThinkingLevel {
        if let Some(ref def) = self.agent_definition {
            def.effective_thinking_level()
        } else {
            self.thinking_level
        }
    }

    /// Evaluate permission rules for a tool call.
    ///
    /// Returns `None` if no rules match (caller decides default behavior).
    /// `arg_pattern` is used for tools that have pattern-level rules (e.g. bash commands).
    pub fn evaluate_permission(
        &self,
        tool_name: &str,
        arg_pattern: &str,
    ) -> Option<PermissionAction> {
        use crate::subagents::spec::{glob_match, pattern_specificity};

        if self.permission.is_empty() {
            return None;
        }

        let mut best_match: Option<(PermissionAction, usize)> = None;

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ReactLoopConfig::default();
        assert!(config.max_iterations.is_none());
        assert_eq!(config.max_nudge_attempts, 3);
        assert_eq!(config.max_todo_nudges, 4);
        assert!(config.permission.is_empty());
    }

    #[test]
    fn test_evaluate_permission_empty_rules() {
        let config = ReactLoopConfig::default();
        assert!(config.evaluate_permission("read_file", "").is_none());
    }

    #[test]
    fn test_evaluate_permission_with_action_rule() {
        let mut config = ReactLoopConfig::default();
        config.permission.insert(
            "run_command".to_string(),
            PermissionRule::Action(PermissionAction::Deny),
        );
        assert_eq!(
            config.evaluate_permission("run_command", ""),
            Some(PermissionAction::Deny)
        );
    }

    #[test]
    fn test_evaluate_permission_no_match() {
        let mut config = ReactLoopConfig::default();
        config.permission.insert(
            "run_command".to_string(),
            PermissionRule::Action(PermissionAction::Deny),
        );
        assert!(config.evaluate_permission("read_file", "").is_none());
    }
}
