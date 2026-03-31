//! Cross-iteration mutable state for the ReAct loop.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::doom_loop::DoomLoopDetector;
use crate::prompts::reminders::{
    MessageClass, ProactiveReminderConfig, ProactiveReminderScheduler,
};

/// Mutable state that persists across iterations of the ReAct loop.
///
/// Bundled into a struct to keep the orchestrator loop clean and make
/// dependencies explicit when passing to phase functions.
pub(super) struct LoopState {
    pub iteration: usize,
    pub consecutive_no_tool_calls: usize,
    pub consecutive_truncations: usize,
    pub doom_detector: DoomLoopDetector,

    /// Per-subdirectory instruction injection tracker.
    pub subdir_tracker: opendev_context::SubdirInstructionTracker,
    /// Startup instruction paths — kept for `reset_after_compaction()`.
    pub startup_paths: Vec<PathBuf>,

    /// Skill-driven model override from frontmatter.
    pub skill_model_override: Option<String>,

    /// Session-level auto-approved command prefixes / MCP tool names.
    pub auto_approved_patterns: HashSet<String>,

    // Nudge/reminder state
    pub todo_nudge_count: usize,
    pub all_todos_complete_nudged: bool,
    pub completion_nudge_sent: bool,
    pub consecutive_reads: usize,
    pub proactive_reminders: ProactiveReminderScheduler,
}

impl LoopState {
    /// Create a new `LoopState` for a fresh react loop execution.
    pub fn new(working_dir: &std::path::Path) -> Self {
        let startup_paths: Vec<PathBuf> = opendev_context::discover_instruction_files(working_dir)
            .into_iter()
            .map(|f| f.path)
            .collect();
        let subdir_tracker = opendev_context::SubdirInstructionTracker::new(
            working_dir.to_path_buf(),
            &startup_paths,
        );

        Self {
            iteration: 0,
            consecutive_no_tool_calls: 0,
            consecutive_truncations: 0,
            doom_detector: DoomLoopDetector::new(),
            subdir_tracker,
            startup_paths,
            skill_model_override: None,
            auto_approved_patterns: HashSet::new(),
            todo_nudge_count: 0,
            all_todos_complete_nudged: false,
            completion_nudge_sent: false,
            consecutive_reads: 0,
            proactive_reminders: ProactiveReminderScheduler::new(vec![
                ProactiveReminderConfig {
                    name: "todo_proactive_reminder",
                    turns_since_reset: 10,
                    turns_between: 10,
                    class: MessageClass::Nudge,
                },
                ProactiveReminderConfig {
                    name: "task_proactive_reminder",
                    turns_since_reset: 10,
                    turns_between: 10,
                    class: MessageClass::Nudge,
                },
            ]),
        }
    }
}
