use serde::{Deserialize, Serialize};

/// Classification of how an agent can be used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    /// Main agent for top-level conversations.
    Primary,
    /// Can only be spawned as a subagent.
    #[default]
    Subagent,
    /// Can function in both primary and subagent roles.
    All,
}

impl AgentMode {
    pub(super) fn default_mode() -> Self {
        Self::default()
    }

    /// Parse a mode string, defaulting to `Subagent` for unknown values.
    pub fn parse_mode(s: &str) -> Self {
        match s {
            "primary" => Self::Primary,
            "all" => Self::All,
            _ => Self::Subagent,
        }
    }

    /// Whether this agent can be spawned as a subagent.
    pub fn can_be_subagent(&self) -> bool {
        matches!(self, Self::Subagent | Self::All)
    }

    /// Whether this agent can serve as a primary agent.
    pub fn can_be_primary(&self) -> bool {
        matches!(self, Self::Primary | Self::All)
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::SubAgentSpec;
    use super::*;

    #[test]
    fn test_agent_mode_default() {
        assert_eq!(AgentMode::default(), AgentMode::Subagent);
    }

    #[test]
    fn test_agent_mode_from_str() {
        assert_eq!(AgentMode::parse_mode("primary"), AgentMode::Primary);
        assert_eq!(AgentMode::parse_mode("subagent"), AgentMode::Subagent);
        assert_eq!(AgentMode::parse_mode("all"), AgentMode::All);
        assert_eq!(AgentMode::parse_mode("unknown"), AgentMode::Subagent);
    }

    #[test]
    fn test_agent_mode_capabilities() {
        assert!(AgentMode::Primary.can_be_primary());
        assert!(!AgentMode::Primary.can_be_subagent());

        assert!(!AgentMode::Subagent.can_be_primary());
        assert!(AgentMode::Subagent.can_be_subagent());

        assert!(AgentMode::All.can_be_primary());
        assert!(AgentMode::All.can_be_subagent());
    }

    #[test]
    fn test_agent_mode_serde() {
        let spec = SubAgentSpec::new("test", "desc", "prompt").with_mode(AgentMode::Primary);

        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("\"primary\""));

        let restored: SubAgentSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.mode, AgentMode::Primary);
    }
}
