//! Agents tool — list and spawn subagent types.
//!
//! Provides two tools:
//! - `agents` — List available subagent configurations
//! - `spawn_subagent` — Spawn a subagent to handle an isolated task
//!
//! Mirrors `opendev/core/context_engineering/tools/implementations/agents_tool.py`
//! and the subagent spawning logic from the Python react loop.

mod events;
mod list;
mod spawn;

pub use events::{ChannelProgressCallback, SubagentEvent};
pub use list::AgentsTool;
pub use spawn::SpawnSubagentTool;
