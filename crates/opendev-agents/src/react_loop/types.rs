//! Types and constants for the ReAct loop.

use serde_json::Value;

/// Metrics for a single tool call execution.
#[derive(Debug, Clone)]
pub struct ToolCallMetric {
    /// Name of the tool that was called.
    pub tool_name: String,
    /// Wall-clock duration of the tool execution in milliseconds.
    pub duration_ms: u64,
    /// Whether the tool call succeeded.
    pub success: bool,
}

/// Per-iteration metrics collected during the ReAct loop.
#[derive(Debug, Clone, Default)]
pub struct IterationMetrics {
    /// 1-based iteration number.
    pub iteration: usize,
    /// Wall-clock latency of the LLM API call in milliseconds.
    pub llm_latency_ms: u64,
    /// Number of input (prompt) tokens consumed.
    pub input_tokens: u64,
    /// Number of output (completion) tokens generated.
    pub output_tokens: u64,
    /// Metrics for each tool call executed in this iteration.
    pub tool_calls: Vec<ToolCallMetric>,
    /// Total wall-clock duration of the iteration in milliseconds.
    pub total_duration_ms: u64,
}

/// Tools that are safe for parallel execution (read-only, no side effects).
pub static PARALLELIZABLE_TOOLS: &[&str] = &[
    "read_file",
    "list_files",
    "search",
    "fetch_url",
    "web_search",
    "capture_web_screenshot",
    "analyze_image",
    "list_todos",
    "search_tools",
    "find_symbol",
    "find_referencing_symbols",
];

/// Extended readonly set for thinking-skip heuristic.
/// Matches Python's `IterationMixin._READONLY_TOOLS`.
pub(super) static READONLY_TOOLS: &[&str] = &[
    "read_file",
    "list_files",
    "search",
    "fetch_url",
    "web_search",
    "find_symbol",
    "find_referencing_symbols",
    "list_todos",
    "search_tools",
    "analyze_image",
    "capture_screenshot",
    "capture_web_screenshot",
    "list_sessions",
    "get_session_history",
    "list_subagents",
    "memory_search",
    "list_agents",
];

/// Read-only tool names for consecutive-reads detection.
/// When all tool calls in an iteration are from this set, the consecutive reads
/// counter increments. After 5 consecutive read-only iterations, a nudge is injected.
pub(super) static READ_OPS: &[&str] = &[
    "read_file",
    "list_files",
    "search",
    "fetch_url",
    "web_search",
    "find_symbol",
    "list_todos",
    "read_pdf",
    "analyze_image",
];

/// Result of processing a single turn in the ReAct loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnResult {
    /// The loop should continue with the next iteration.
    Continue,
    /// The agent wants to execute tool calls.
    ToolCall {
        /// Tool call objects from the LLM response.
        tool_calls: Vec<Value>,
    },
    /// The agent has completed its task.
    Complete {
        /// Final content from the agent.
        content: String,
        /// Completion status (e.g. "success", "failed").
        status: Option<String>,
    },
    /// Maximum iterations reached.
    MaxIterations,
    /// The run was interrupted by the user.
    Interrupted,
}
