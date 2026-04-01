//! Tool category enums and display entry types.

use super::style_tokens;
use ratatui::style::Color;

/// Tool category for grouping purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// File read operations (read_file, read_pdf, list_files).
    FileRead,
    /// File write/edit operations (write_file, edit_file).
    FileWrite,
    /// Bash/command execution.
    Bash,
    /// Search operations (search, web_search).
    Search,
    /// Web operations (fetch_url, open_browser, screenshots).
    Web,
    /// Subagent/agent spawn operations.
    Agent,
    /// Symbol/LSP operations (find_symbol, rename_symbol).
    Symbol,
    /// MCP tool calls.
    Mcp,
    /// Plan/task management tools.
    Plan,
    /// Docker operations.
    Docker,
    /// User interaction (ask_user).
    UserInteraction,
    /// Notebook operations.
    Notebook,
    /// Unknown/other tools.
    Other,
}

/// Which result formatter to use for a tool's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultFormat {
    Bash,
    File,
    Directory,
    Generic,
    Todo,
}

/// Single source of truth for how a tool appears in the TUI.
pub struct ToolDisplayEntry {
    /// Tool name(s) this entry matches (exact match).
    pub names: &'static [&'static str],
    /// Category for grouping.
    pub category: ToolCategory,
    /// Display verb shown in TUI, e.g. "Read", "Bash".
    pub verb: &'static str,
    /// Fallback noun when no arg is available, e.g. "file", "command".
    pub label: &'static str,
    /// Ordered keys to try when extracting the primary arg for display.
    pub primary_arg_keys: &'static [&'static str],
    /// Which result formatter to use.
    pub result_format: ResultFormat,
}

/// Map a category name string to a `ToolCategory` enum variant.
pub(crate) fn category_from_name(name: &str) -> ToolCategory {
    match name {
        "FileRead" => ToolCategory::FileRead,
        "FileWrite" => ToolCategory::FileWrite,
        "Bash" => ToolCategory::Bash,
        "Search" => ToolCategory::Search,
        "Web" => ToolCategory::Web,
        "Agent" => ToolCategory::Agent,
        "Symbol" => ToolCategory::Symbol,
        "Mcp" => ToolCategory::Mcp,
        "Plan" => ToolCategory::Plan,
        "Docker" => ToolCategory::Docker,
        "UserInteraction" => ToolCategory::UserInteraction,
        "Notebook" => ToolCategory::Notebook,
        _ => ToolCategory::Other,
    }
}

/// Classify a tool name into its category.
pub fn categorize_tool(tool_name: &str) -> ToolCategory {
    super::tool_entries::lookup_tool(tool_name).category
}

/// Get the primary display color for a tool category.
///
/// All tools use orange (WARNING) for unified appearance.
pub fn tool_color(_category: ToolCategory) -> Color {
    style_tokens::WARNING
}

/// Human-friendly display name for a tool.
///
/// Returns `(verb, label)`.
pub fn tool_display_parts(tool_name: &str) -> (&'static str, &'static str) {
    let entry = super::tool_entries::lookup_tool(tool_name);
    (entry.verb, entry.label)
}
