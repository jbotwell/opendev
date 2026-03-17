//! Operation mode and autonomy level enums.

/// Operation mode — mirrors `OperationMode` from the Python side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationMode {
    Normal,
    Plan,
}

impl std::fmt::Display for OperationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "Normal"),
            Self::Plan => write!(f, "Plan"),
        }
    }
}

impl OperationMode {
    /// Parse from string (case-insensitive).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "plan" => Some(Self::Plan),
            _ => None,
        }
    }
}

/// Autonomy level — mirrors Python `StatusBar.autonomy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutonomyLevel {
    Manual,
    SemiAuto,
    Auto,
}

impl std::fmt::Display for AutonomyLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manual => write!(f, "Manual"),
            Self::SemiAuto => write!(f, "Semi-Auto"),
            Self::Auto => write!(f, "Auto"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_mode_display() {
        assert_eq!(OperationMode::Normal.to_string(), "Normal");
        assert_eq!(OperationMode::Plan.to_string(), "Plan");
    }

    #[test]
    fn test_operation_mode_from_str_loose() {
        assert_eq!(
            OperationMode::from_str_loose("plan"),
            Some(OperationMode::Plan)
        );
        assert_eq!(
            OperationMode::from_str_loose("Normal"),
            Some(OperationMode::Normal)
        );
        assert_eq!(OperationMode::from_str_loose("bogus"), None);
    }

    #[test]
    fn test_autonomy_level_from_str_loose() {
        assert_eq!(
            AutonomyLevel::from_str_loose("auto"),
            Some(AutonomyLevel::Auto)
        );
        assert_eq!(
            AutonomyLevel::from_str_loose("Semi-Auto"),
            Some(AutonomyLevel::SemiAuto)
        );
        assert_eq!(
            AutonomyLevel::from_str_loose("manual"),
            Some(AutonomyLevel::Manual)
        );
        assert_eq!(AutonomyLevel::from_str_loose("bogus"), None);
    }
}
