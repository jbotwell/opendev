//! Skill metadata and loaded skill types.

use std::path::PathBuf;

/// Where a skill was loaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    /// Compiled into the binary.
    Builtin,
    /// From `~/.opendev/skills/`.
    UserGlobal,
    /// From `<project>/.opendev/skills/`.
    Project,
    /// Downloaded from a remote URL.
    Url(String),
}

impl std::fmt::Display for SkillSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillSource::Builtin => write!(f, "builtin"),
            SkillSource::UserGlobal => write!(f, "user-global"),
            SkillSource::Project => write!(f, "project"),
            SkillSource::Url(url) => write!(f, "url:{url}"),
        }
    }
}

/// Metadata extracted from a skill file's YAML frontmatter.
#[derive(Debug, Clone)]
pub struct SkillMetadata {
    /// Skill name (e.g. `"commit"`).
    pub name: String,
    /// Human-readable description, ideally starting with "Use when...".
    pub description: String,
    /// Namespace for grouping (default: `"default"`).
    pub namespace: String,
    /// Path to the source `.md` file on disk (None for builtins).
    pub path: Option<PathBuf>,
    /// Where this skill was discovered.
    pub source: SkillSource,
    /// Optional model override for this skill (e.g. `"gpt-4o"`, `"claude-sonnet-4-5-20250514"`).
    /// When set, the agent should use this model instead of the default when executing the skill.
    pub model: Option<String>,
    /// Optional agent override for this skill.
    /// When set, the skill should be executed by the specified agent instead of the current one.
    pub agent: Option<String>,
}

impl SkillMetadata {
    /// Build the full name including namespace prefix.
    ///
    /// Returns `"name"` for default namespace, `"namespace:name"` otherwise.
    pub fn full_name(&self) -> String {
        if self.namespace == "default" {
            self.name.clone()
        } else {
            format!("{}:{}", self.namespace, self.name)
        }
    }

    /// Estimate token count for the skill file.
    ///
    /// Uses a rough heuristic of ~4 characters per token.
    pub fn estimate_tokens(&self) -> Option<usize> {
        if let Some(path) = &self.path
            && let Ok(content) = std::fs::read_to_string(path)
        {
            return Some(content.len() / 4);
        }
        None
    }
}

/// A companion file discovered alongside a directory-style skill.
#[derive(Debug, Clone)]
pub struct CompanionFile {
    /// Absolute path to the file.
    pub path: PathBuf,
    /// Path relative to the skill directory.
    pub relative_path: String,
}

/// A fully loaded skill with its content ready for injection.
#[derive(Debug, Clone)]
pub struct LoadedSkill {
    /// Metadata from the frontmatter.
    pub metadata: SkillMetadata,
    /// The markdown body content (frontmatter stripped).
    pub content: String,
    /// Companion files found alongside the skill (for directory-style skills).
    pub companion_files: Vec<CompanionFile>,
}

impl LoadedSkill {
    /// Estimate the token count of the loaded content.
    pub fn estimate_tokens(&self) -> usize {
        self.content.len() / 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_metadata(name: &str, namespace: &str, source: SkillSource) -> SkillMetadata {
        SkillMetadata {
            name: name.to_string(),
            description: format!("Use when {name}"),
            namespace: namespace.to_string(),
            path: None,
            source,
            model: None,
            agent: None,
        }
    }

    // --- SkillSource ---

    #[test]
    fn test_skill_source_display() {
        assert_eq!(SkillSource::Builtin.to_string(), "builtin");
        assert_eq!(SkillSource::UserGlobal.to_string(), "user-global");
        assert_eq!(SkillSource::Project.to_string(), "project");
        assert_eq!(
            SkillSource::Url("https://example.com/skills".to_string()).to_string(),
            "url:https://example.com/skills"
        );
    }

    #[test]
    fn test_skill_source_equality() {
        assert_eq!(SkillSource::Builtin, SkillSource::Builtin);
        assert_ne!(SkillSource::Builtin, SkillSource::Project);
        assert_eq!(
            SkillSource::Url("a".to_string()),
            SkillSource::Url("a".to_string())
        );
        assert_ne!(
            SkillSource::Url("a".to_string()),
            SkillSource::Url("b".to_string())
        );
    }

    // --- SkillMetadata::full_name ---

    #[test]
    fn test_full_name_default_namespace() {
        let m = make_metadata("commit", "default", SkillSource::Builtin);
        assert_eq!(m.full_name(), "commit");
    }

    #[test]
    fn test_full_name_custom_namespace() {
        let m = make_metadata("deploy", "devops", SkillSource::Project);
        assert_eq!(m.full_name(), "devops:deploy");
    }

    #[test]
    fn test_full_name_empty_namespace_is_not_default() {
        let m = make_metadata("test", "", SkillSource::Builtin);
        // Empty string != "default", so it should prefix
        assert_eq!(m.full_name(), ":test");
    }

    // --- SkillMetadata::estimate_tokens ---

    #[test]
    fn test_estimate_tokens_no_path() {
        let m = make_metadata("commit", "default", SkillSource::Builtin);
        assert_eq!(m.estimate_tokens(), None);
    }

    #[test]
    fn test_estimate_tokens_missing_file() {
        let mut m = make_metadata("commit", "default", SkillSource::Project);
        m.path = Some(PathBuf::from("/nonexistent/skill.md"));
        assert_eq!(m.estimate_tokens(), None);
    }

    #[test]
    fn test_estimate_tokens_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("skill.md");
        // 400 chars → ~100 tokens
        std::fs::write(&file, "x".repeat(400)).unwrap();

        let mut m = make_metadata("test", "default", SkillSource::Project);
        m.path = Some(file);
        assert_eq!(m.estimate_tokens(), Some(100));
    }

    // --- LoadedSkill ---

    #[test]
    fn test_loaded_skill_estimate_tokens() {
        let skill = LoadedSkill {
            metadata: make_metadata("commit", "default", SkillSource::Builtin),
            content: "a".repeat(200),
            companion_files: vec![],
        };
        assert_eq!(skill.estimate_tokens(), 50);
    }

    #[test]
    fn test_loaded_skill_estimate_tokens_empty() {
        let skill = LoadedSkill {
            metadata: make_metadata("empty", "default", SkillSource::Builtin),
            content: String::new(),
            companion_files: vec![],
        };
        assert_eq!(skill.estimate_tokens(), 0);
    }

    #[test]
    fn test_loaded_skill_estimate_tokens_small() {
        let skill = LoadedSkill {
            metadata: make_metadata("small", "default", SkillSource::Builtin),
            content: "hi".to_string(), // 2 chars → 0 tokens (integer division)
            companion_files: vec![],
        };
        assert_eq!(skill.estimate_tokens(), 0);
    }
}
