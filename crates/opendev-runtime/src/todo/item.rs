use serde::{Deserialize, Serialize};

use super::TodoStatus;

/// A single todo item derived from a plan step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// Unique ID within the todo list (1-based index).
    pub id: usize,
    /// Short title (the plan step text).
    pub title: String,
    /// Current status.
    pub status: TodoStatus,
    /// Present continuous text for spinner display (e.g., "Running tests").
    #[serde(default)]
    pub active_form: String,
    /// Completion notes / log entry.
    #[serde(default)]
    pub log: String,
    /// When the item was created.
    pub created_at: String,
    /// When the status last changed.
    pub updated_at: String,
}
