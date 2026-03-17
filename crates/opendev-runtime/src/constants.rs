//! Shared constants for the approval system.
//!
//! Provides canonical definitions for safe commands and autonomy levels
//! used by both TUI and Web UI approval managers.
//!
//! Ported from `opendev/core/runtime/approval/constants.py`.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Autonomy levels for command approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum AutonomyLevel {
    /// Every command requires manual approval.
    #[serde(rename = "Manual")]
    Manual,
    /// Safe commands auto-approved; others require approval.
    #[serde(rename = "Semi-Auto")]
    #[default]
    SemiAuto,
    /// All commands auto-approved (dangerous still flagged).
    #[serde(rename = "Auto")]
    Auto,
}

impl fmt::Display for AutonomyLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AutonomyLevel::Manual => write!(f, "Manual"),
            AutonomyLevel::SemiAuto => write!(f, "Semi-Auto"),
            AutonomyLevel::Auto => write!(f, "Auto"),
        }
    }
}

impl AutonomyLevel {
    /// Parse from string (case-insensitive).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "manual" => Some(Self::Manual),
            "semi-auto" | "semiauto" | "semi" => Some(Self::SemiAuto),
            "auto" | "full" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// Safe commands that can be auto-approved in Semi-Auto mode.
///
/// Shared between TUI and Web approval managers.
pub const SAFE_COMMANDS: &[&str] = &[
    "ls",
    "cat",
    "head",
    "tail",
    "grep",
    "find",
    "wc",
    "pwd",
    "echo",
    "which",
    "type",
    "file",
    "stat",
    "du",
    "df",
    "tree",
    "git status",
    "git log",
    "git diff",
    "git branch",
    "git show",
    "git remote",
    "git tag",
    "git stash list",
    "python --version",
    "python3 --version",
    "node --version",
    "npm --version",
    "cargo --version",
    "go version",
];

/// Check if a command is considered safe for auto-approval.
///
/// Uses strict matching: the command must either equal a safe command exactly
/// or start with it followed by a space (preventing e.g. `cat` from matching
/// `catastrophe`).
pub fn is_safe_command(command: &str) -> bool {
    if command.is_empty() {
        return false;
    }
    let cmd_lower = command.trim().to_lowercase();
    SAFE_COMMANDS.iter().any(|safe| {
        let safe_lower = safe.to_lowercase();
        cmd_lower == safe_lower || cmd_lower.starts_with(&format!("{safe_lower} "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_safe_command() {
        assert!(is_safe_command("ls"));
        assert!(is_safe_command("ls -la"));
        assert!(is_safe_command("git status"));
        assert!(is_safe_command("git diff --staged"));
        assert!(is_safe_command("cat foo.txt"));
        assert!(!is_safe_command("rm -rf /"));
        assert!(!is_safe_command("catastrophe")); // must not match "cat"
        assert!(!is_safe_command(""));
    }

    #[test]
    fn test_safe_command_case_insensitive() {
        assert!(is_safe_command("LS -la"));
        assert!(is_safe_command("Git Status"));
    }

    #[test]
    fn test_autonomy_level_display() {
        assert_eq!(AutonomyLevel::Manual.to_string(), "Manual");
        assert_eq!(AutonomyLevel::SemiAuto.to_string(), "Semi-Auto");
        assert_eq!(AutonomyLevel::Auto.to_string(), "Auto");
    }

    #[test]
    fn test_autonomy_level_parse() {
        assert_eq!(
            AutonomyLevel::from_str_loose("manual"),
            Some(AutonomyLevel::Manual)
        );
        assert_eq!(
            AutonomyLevel::from_str_loose("Semi-Auto"),
            Some(AutonomyLevel::SemiAuto)
        );
        assert_eq!(
            AutonomyLevel::from_str_loose("auto"),
            Some(AutonomyLevel::Auto)
        );
        assert_eq!(AutonomyLevel::from_str_loose("garbage"), None);
    }

    #[test]
    fn test_autonomy_level_serde_roundtrip() {
        let level = AutonomyLevel::SemiAuto;
        let json = serde_json::to_string(&level).unwrap();
        assert_eq!(json, "\"Semi-Auto\"");
        let deserialized: AutonomyLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, level);
    }
}
