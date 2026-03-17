//! Fine-grained permission rule set with glob-based matching and directory scoping.
//!
//! Provides [`PermissionRuleSet`] for ordered, priority-based permission evaluation,
//! with optional per-directory scoping via glob patterns.

mod glob;

pub use glob::{glob_matches, glob_matches_path};

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Action to take when a permission rule matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAction {
    /// Silently allow the operation.
    Allow,
    /// Silently deny the operation.
    Deny,
    /// Prompt the user for confirmation.
    Prompt,
}

/// A single permission rule with glob pattern matching and optional directory scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    /// Glob pattern matched against `"tool_name:args"` (e.g. `"bash:rm *"`, `"edit:*"`).
    pub pattern: String,
    /// What to do when the pattern matches.
    pub action: PermissionAction,
    /// Higher-priority rules are evaluated first.
    pub priority: i32,
    /// Optional glob restricting the rule to operations within matching directories.
    /// Example: `Some("src/**")` only applies when the working directory is under `src/`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory_scope: Option<String>,
}

/// An ordered collection of permission rules evaluated highest-priority-first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionRuleSet {
    rules: Vec<PermissionRule>,
}

/// Check whether a file path points to a sensitive file that should be denied by default.
///
/// Returns `true` for `.env`, `.env.*` (but NOT `.env.example`), and other credential files.
pub fn is_sensitive_file(path: &str) -> bool {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let lower = filename.to_lowercase();

    // .env and .env.* (but allow .env.example, .env.sample, .env.template)
    if lower == ".env" {
        return true;
    }
    if let Some(suffix) = lower.strip_prefix(".env.") {
        return !matches!(suffix, "example" | "sample" | "template");
    }

    // Other common credential files
    matches!(
        lower.as_str(),
        "credentials.json"
            | "service-account.json"
            | "id_rsa"
            | "id_ed25519"
            | ".npmrc"
            | ".pypirc"
    )
}

impl PermissionRuleSet {
    /// Create an empty rule set.
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Create a rule set with built-in security defaults.
    ///
    /// Includes auto-deny for reading/writing `.env` files and other credential files,
    /// while allowing `.env.example`.
    pub fn with_defaults() -> Self {
        let mut rs = Self::new();

        // Deny reading sensitive env files (high priority)
        rs.add_rule(PermissionRule {
            pattern: "read_file:*.env".into(),
            action: PermissionAction::Deny,
            priority: 1000,
            directory_scope: None,
        });
        rs.add_rule(PermissionRule {
            pattern: "read_file:*.env.*".into(),
            action: PermissionAction::Deny,
            priority: 1000,
            directory_scope: None,
        });
        // Allow .env.example specifically (higher priority overrides deny)
        rs.add_rule(PermissionRule {
            pattern: "read_file:*.env.example".into(),
            action: PermissionAction::Allow,
            priority: 1001,
            directory_scope: None,
        });
        rs.add_rule(PermissionRule {
            pattern: "read_file:*.env.sample".into(),
            action: PermissionAction::Allow,
            priority: 1001,
            directory_scope: None,
        });
        rs.add_rule(PermissionRule {
            pattern: "read_file:*.env.template".into(),
            action: PermissionAction::Allow,
            priority: 1001,
            directory_scope: None,
        });

        // Deny editing/writing sensitive env files
        rs.add_rule(PermissionRule {
            pattern: "edit_file:*.env".into(),
            action: PermissionAction::Deny,
            priority: 1000,
            directory_scope: None,
        });
        rs.add_rule(PermissionRule {
            pattern: "edit_file:*.env.*".into(),
            action: PermissionAction::Deny,
            priority: 1000,
            directory_scope: None,
        });
        rs.add_rule(PermissionRule {
            pattern: "write_file:*.env".into(),
            action: PermissionAction::Deny,
            priority: 1000,
            directory_scope: None,
        });
        rs.add_rule(PermissionRule {
            pattern: "write_file:*.env.*".into(),
            action: PermissionAction::Deny,
            priority: 1000,
            directory_scope: None,
        });

        rs
    }

    /// Add a rule to the set.
    pub fn add_rule(&mut self, rule: PermissionRule) {
        self.rules.push(rule);
    }

    /// Remove all rules matching a predicate.
    pub fn remove_rules<F: Fn(&PermissionRule) -> bool>(&mut self, predicate: F) {
        self.rules.retain(|r| !predicate(r));
    }

    /// Read-only access to the rules.
    pub fn rules(&self) -> &[PermissionRule] {
        &self.rules
    }

    /// Evaluate a tool invocation against the rule set.
    ///
    /// `tool_name` is the tool being invoked (e.g. `"bash"`, `"edit"`).
    /// `args` is the argument string (e.g. the command or file path).
    /// `working_dir` is the optional directory context for directory-scoped rules.
    ///
    /// Returns the action from the highest-priority matching rule, or `None` if
    /// no rule matches.
    pub fn evaluate(
        &self,
        tool_name: &str,
        args: &str,
        working_dir: Option<&Path>,
    ) -> Option<PermissionAction> {
        let input = format!("{tool_name}:{args}");

        let mut sorted: Vec<&PermissionRule> = self.rules.iter().collect();
        sorted.sort_by(|a, b| b.priority.cmp(&a.priority));

        for rule in sorted {
            // Check directory scope first
            if let Some(ref scope) = rule.directory_scope {
                match working_dir {
                    Some(dir) => {
                        if !glob_matches_path(scope, &dir.to_string_lossy()) {
                            continue;
                        }
                    }
                    None => continue, // scoped rule requires a directory
                }
            }

            if glob_matches(&rule.pattern, &input) {
                return Some(rule.action.clone());
            }
        }

        None
    }

    /// Convenience wrapper without directory context.
    pub fn evaluate_simple(&self, tool_name: &str, args: &str) -> Option<PermissionAction> {
        self.evaluate(tool_name, args, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_ruleset_empty_returns_none() {
        let rs = PermissionRuleSet::new();
        assert_eq!(rs.evaluate_simple("bash", "ls"), None);
    }

    #[test]
    fn test_ruleset_basic_allow() {
        let mut rs = PermissionRuleSet::new();
        rs.add_rule(PermissionRule {
            pattern: "bash:*".into(),
            action: PermissionAction::Allow,
            priority: 10,
            directory_scope: None,
        });
        assert_eq!(
            rs.evaluate_simple("bash", "ls -la"),
            Some(PermissionAction::Allow)
        );
        assert_eq!(rs.evaluate_simple("edit", "foo.rs"), None);
    }

    #[test]
    fn test_ruleset_priority_ordering() {
        let mut rs = PermissionRuleSet::new();
        rs.add_rule(PermissionRule {
            pattern: "bash:*".into(),
            action: PermissionAction::Allow,
            priority: 1,
            directory_scope: None,
        });
        rs.add_rule(PermissionRule {
            pattern: "bash:rm *".into(),
            action: PermissionAction::Deny,
            priority: 10,
            directory_scope: None,
        });
        // "rm -rf /" matches the Deny rule with higher priority
        assert_eq!(
            rs.evaluate_simple("bash", "rm -rf /"),
            Some(PermissionAction::Deny)
        );
        // "ls" only matches the Allow rule
        assert_eq!(
            rs.evaluate_simple("bash", "ls"),
            Some(PermissionAction::Allow)
        );
    }

    #[test]
    fn test_ruleset_directory_scope() {
        let mut rs = PermissionRuleSet::new();
        rs.add_rule(PermissionRule {
            pattern: "edit:*".into(),
            action: PermissionAction::Allow,
            priority: 10,
            directory_scope: Some("src/**".into()),
        });
        rs.add_rule(PermissionRule {
            pattern: "edit:*".into(),
            action: PermissionAction::Deny,
            priority: 10,
            directory_scope: Some("vendor/**".into()),
        });

        let src_dir = PathBuf::from("src/components/button.rs");
        let vendor_dir = PathBuf::from("vendor/lib/foo.rs");

        assert_eq!(
            rs.evaluate("edit", "foo.rs", Some(&src_dir)),
            Some(PermissionAction::Allow)
        );
        assert_eq!(
            rs.evaluate("edit", "foo.rs", Some(&vendor_dir)),
            Some(PermissionAction::Deny)
        );
        // No directory => scoped rules don't match
        assert_eq!(rs.evaluate("edit", "foo.rs", None), None);
    }

    #[test]
    fn test_ruleset_scoped_and_unscoped_mix() {
        let mut rs = PermissionRuleSet::new();
        // Low-priority blanket allow
        rs.add_rule(PermissionRule {
            pattern: "edit:*".into(),
            action: PermissionAction::Allow,
            priority: 1,
            directory_scope: None,
        });
        // High-priority deny for vendor
        rs.add_rule(PermissionRule {
            pattern: "edit:*".into(),
            action: PermissionAction::Deny,
            priority: 100,
            directory_scope: Some("vendor/**".into()),
        });

        let vendor = PathBuf::from("vendor/lib.rs");
        let src = PathBuf::from("src/main.rs");

        // vendor => Deny wins
        assert_eq!(
            rs.evaluate("edit", "x", Some(&vendor)),
            Some(PermissionAction::Deny)
        );
        // src => scoped Deny doesn't match, blanket Allow applies
        assert_eq!(
            rs.evaluate("edit", "x", Some(&src)),
            Some(PermissionAction::Allow)
        );
    }

    #[test]
    fn test_ruleset_prompt_action() {
        let mut rs = PermissionRuleSet::new();
        rs.add_rule(PermissionRule {
            pattern: "bash:sudo *".into(),
            action: PermissionAction::Prompt,
            priority: 50,
            directory_scope: None,
        });
        assert_eq!(
            rs.evaluate_simple("bash", "sudo rm -rf /"),
            Some(PermissionAction::Prompt)
        );
    }

    #[test]
    fn test_is_sensitive_file() {
        // .env files
        assert!(is_sensitive_file(".env"));
        assert!(is_sensitive_file("/path/to/.env"));
        assert!(is_sensitive_file(".env.local"));
        assert!(is_sensitive_file(".env.production"));
        assert!(is_sensitive_file("/app/.env.staging"));

        // Allowed .env variants
        assert!(!is_sensitive_file(".env.example"));
        assert!(!is_sensitive_file(".env.sample"));
        assert!(!is_sensitive_file(".env.template"));

        // Other credential files
        assert!(is_sensitive_file("credentials.json"));
        assert!(is_sensitive_file("id_rsa"));
        assert!(is_sensitive_file("id_ed25519"));
        assert!(is_sensitive_file(".npmrc"));
        assert!(is_sensitive_file(".pypirc"));

        // Non-sensitive files
        assert!(!is_sensitive_file("main.rs"));
        assert!(!is_sensitive_file("Cargo.toml"));
        assert!(!is_sensitive_file("README.md"));
        assert!(!is_sensitive_file(".envrc")); // not .env
    }

    #[test]
    fn test_defaults_deny_env_files() {
        let rs = PermissionRuleSet::with_defaults();

        // .env denied
        assert_eq!(
            rs.evaluate_simple("read_file", "/app/.env"),
            Some(PermissionAction::Deny)
        );
        assert_eq!(
            rs.evaluate_simple("read_file", "/app/.env.local"),
            Some(PermissionAction::Deny)
        );
        assert_eq!(
            rs.evaluate_simple("edit_file", ".env"),
            Some(PermissionAction::Deny)
        );
        assert_eq!(
            rs.evaluate_simple("write_file", "/app/.env.production"),
            Some(PermissionAction::Deny)
        );

        // .env.example allowed
        assert_eq!(
            rs.evaluate_simple("read_file", "/app/.env.example"),
            Some(PermissionAction::Allow)
        );
        assert_eq!(
            rs.evaluate_simple("read_file", ".env.sample"),
            Some(PermissionAction::Allow)
        );

        // Normal files unaffected
        assert_eq!(rs.evaluate_simple("read_file", "main.rs"), None);
    }

    #[test]
    fn test_ruleset_remove_rules() {
        let mut rs = PermissionRuleSet::new();
        rs.add_rule(PermissionRule {
            pattern: "bash:*".into(),
            action: PermissionAction::Allow,
            priority: 1,
            directory_scope: None,
        });
        rs.add_rule(PermissionRule {
            pattern: "edit:*".into(),
            action: PermissionAction::Deny,
            priority: 1,
            directory_scope: None,
        });
        assert_eq!(rs.rules().len(), 2);
        rs.remove_rules(|r| r.action == PermissionAction::Deny);
        assert_eq!(rs.rules().len(), 1);
        assert_eq!(rs.rules()[0].pattern, "bash:*");
    }
}
