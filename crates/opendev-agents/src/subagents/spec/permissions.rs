use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Action to take when a permission rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionAction {
    /// Allow the tool call without user approval.
    Allow,
    /// Deny the tool call entirely.
    Deny,
    /// Prompt the user for approval.
    Ask,
}

/// A permission rule for a tool — either a blanket action or pattern-specific.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PermissionRule {
    /// Single action applies to all patterns for this tool.
    Action(PermissionAction),
    /// Map of glob patterns to actions.
    /// Example: `{ "*": "ask", "git *": "allow", "rm -rf *": "deny" }`
    Patterns(HashMap<String, PermissionAction>),
}

/// Compute specificity of a glob pattern (higher = more specific).
///
/// `"*"` → 0, `"git *"` → 4, `"git status"` → 10 (exact match).
/// Patterns with fewer wildcards and more literal characters are more specific.
pub(crate) fn pattern_specificity(pattern: &str) -> usize {
    if pattern == "*" {
        return 0;
    }
    // Count non-wildcard characters as specificity score.
    pattern.chars().filter(|c| *c != '*' && *c != '?').count()
}

/// Simple glob-style matching: `*` matches any sequence, `?` matches one char.
///
/// Matching is case-sensitive and operates on the full string.
pub(crate) fn glob_match(pattern: &str, input: &str) -> bool {
    let pattern = pattern.as_bytes();
    let input = input.as_bytes();
    let mut pi = 0;
    let mut ii = 0;
    let mut star_pi = usize::MAX;
    let mut star_ii = 0;

    while ii < input.len() {
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == input[ii]) {
            pi += 1;
            ii += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_pi = pi;
            star_ii = ii;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ii += 1;
            ii = star_ii;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }

    pi == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::super::types::SubAgentSpec;
    use super::*;

    #[test]
    fn test_glob_match_basic() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("read_*", "read_file"));
        assert!(glob_match("read_*", "read_dir"));
        assert!(!glob_match("read_*", "write_file"));
        assert!(glob_match("?at", "cat"));
        assert!(!glob_match("?at", "chat"));
        assert!(glob_match("git *", "git status"));
        assert!(glob_match("git *", "git push origin main"));
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("bash", "bash"));
        assert!(!glob_match("bash", "bash2"));
        assert!(!glob_match("bash2", "bash"));
    }

    #[test]
    fn test_permission_action_serde() {
        let action = PermissionAction::Allow;
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(json, "\"allow\"");
        let restored: PermissionAction = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, PermissionAction::Allow);
    }

    #[test]
    fn test_permission_rule_single_action() {
        let rule: PermissionRule = serde_json::from_str("\"deny\"").unwrap();
        assert!(matches!(
            rule,
            PermissionRule::Action(PermissionAction::Deny)
        ));
    }

    #[test]
    fn test_permission_rule_patterns() {
        let json = r#"{"*": "ask", "git *": "allow", "rm -rf *": "deny"}"#;
        let rule: PermissionRule = serde_json::from_str(json).unwrap();
        if let PermissionRule::Patterns(p) = &rule {
            assert_eq!(p.len(), 3);
            assert_eq!(p["*"], PermissionAction::Ask);
            assert_eq!(p["git *"], PermissionAction::Allow);
            assert_eq!(p["rm -rf *"], PermissionAction::Deny);
        } else {
            panic!("Expected Patterns variant");
        }
    }

    #[test]
    fn test_permission_serde_roundtrip() {
        let mut patterns = HashMap::new();
        patterns.insert("*".to_string(), PermissionAction::Ask);
        patterns.insert("git *".to_string(), PermissionAction::Allow);

        let mut perms = HashMap::new();
        perms.insert("bash".to_string(), PermissionRule::Patterns(patterns));
        perms.insert(
            "edit".to_string(),
            PermissionRule::Action(PermissionAction::Deny),
        );

        let spec = SubAgentSpec::new("test", "desc", "prompt").with_permission(perms);

        let json = serde_json::to_string(&spec).unwrap();
        let restored: SubAgentSpec = serde_json::from_str(&json).unwrap();

        assert_eq!(
            restored.evaluate_permission("bash", "git status"),
            Some(PermissionAction::Allow)
        );
        assert_eq!(
            restored.evaluate_permission("edit", "any_file"),
            Some(PermissionAction::Deny)
        );
    }

    #[test]
    fn test_permission_skipped_when_empty() {
        let spec = SubAgentSpec::new("test", "desc", "prompt");
        let json = serde_json::to_string(&spec).unwrap();
        assert!(!json.contains("permission"));
    }
}
