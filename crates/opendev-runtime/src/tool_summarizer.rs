//! Tool result summarizer — creates concise summaries for LLM context.
//!
//! Mirrors Python's `opendev/core/utils/tool_result_summarizer.py`.
//! Summaries are stored in `ToolCall.result_summary` to prevent context
//! bloat while preserving semantic meaning for the LLM.

/// Create a concise 1-2 line summary of a tool result for LLM context.
///
/// This prevents context bloat while maintaining semantic meaning.
/// Summaries are typically 50-200 chars.
pub fn summarize_tool_result(tool_name: &str, output: Option<&str>, error: Option<&str>) -> String {
    // Error case
    if let Some(err) = error {
        let truncated = if err.len() > 200 { &err[..200] } else { err };
        return format!("Error: {truncated}");
    }

    let result_str = output.unwrap_or("");

    if result_str.is_empty() {
        return "Success (no output)".to_string();
    }

    match tool_name {
        // File read operations
        "read_file" | "Read" => {
            let lines = result_str.lines().count();
            let chars = result_str.len();
            format!("Read file ({lines} lines, {chars} chars)")
        }

        // File write operations
        "write_file" | "Write" => "File written successfully".to_string(),

        // File edit operations
        "edit_file" | "Edit" => "File edited successfully".to_string(),

        // Delete operations
        "delete_file" | "Delete" => "File deleted".to_string(),

        // Search operations
        "search" | "Grep" | "file_search" => {
            if result_str.contains("No matches found") || result_str.trim().is_empty() {
                "Search completed (0 matches)".to_string()
            } else {
                let match_count = result_str.lines().count();
                format!("Search completed ({match_count} matches found)")
            }
        }

        // Directory listing
        "list_files" | "list_directory" | "List" => {
            let file_count = if result_str.is_empty() {
                0
            } else {
                result_str.lines().count()
            };
            format!("Listed directory ({file_count} items)")
        }

        // Command execution
        "run_command" | "Run" | "bash_execute" | "Bash" => {
            let lines = result_str.lines().count();
            if lines > 10 {
                format!("Command executed ({lines} lines of output)")
            } else if result_str.len() < 100 {
                format!("Output: {}", &result_str[..result_str.len().min(100)])
            } else {
                "Command executed successfully".to_string()
            }
        }

        // Web operations
        "fetch_url" | "Fetch" | "web_fetch" | "web_search" => {
            "Content fetched successfully".to_string()
        }

        // Screenshot operations
        "capture_screenshot" | "web_screenshot" | "analyze_image" => {
            "Image processed successfully".to_string()
        }

        // Git operations
        "git" => {
            let lines = result_str.lines().count();
            if lines > 10 {
                format!("Git operation completed ({lines} lines)")
            } else if result_str.len() < 100 {
                format!("Output: {}", &result_str[..result_str.len().min(100)])
            } else {
                "Git operation completed".to_string()
            }
        }

        // Todo tools
        "write_todos" => {
            let count = result_str
                .lines()
                .filter(|l| {
                    let t = l.trim();
                    t.starts_with("[todo]") || t.starts_with("[doing]") || t.starts_with("[done]")
                })
                .count();
            if count == 1 {
                "Created 1 todo".to_string()
            } else if count > 1 {
                format!("Created {count} todos")
            } else {
                "Todos updated".to_string()
            }
        }
        "update_todo" => "Todo updated".to_string(),
        "complete_todo" => "Todo completed".to_string(),
        "list_todos" => {
            let count = result_str
                .lines()
                .filter(|l| {
                    let t = l.trim();
                    t.starts_with("[todo]") || t.starts_with("[doing]") || t.starts_with("[done]")
                })
                .count();
            format!("{count} todos listed")
        }
        "clear_todos" => "All todos cleared".to_string(),

        // Generic fallback
        _ => {
            if result_str.len() < 100 {
                result_str.to_string()
            } else {
                let chars = result_str.len();
                let lines = result_str.lines().count();
                format!("Success ({lines} lines, {chars} chars)")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_summary() {
        let summary = summarize_tool_result("read_file", None, Some("file not found"));
        assert_eq!(summary, "Error: file not found");
    }

    #[test]
    fn test_error_truncation() {
        let long_error = "x".repeat(300);
        let summary = summarize_tool_result("read_file", None, Some(&long_error));
        assert!(summary.len() <= 210); // "Error: " + 200 chars
    }

    #[test]
    fn test_empty_output() {
        let summary = summarize_tool_result("read_file", Some(""), None);
        assert_eq!(summary, "Success (no output)");
    }

    #[test]
    fn test_no_output() {
        let summary = summarize_tool_result("write_file", None, None);
        assert_eq!(summary, "Success (no output)");
    }

    #[test]
    fn test_read_file() {
        let output = "line1\nline2\nline3";
        let summary = summarize_tool_result("read_file", Some(output), None);
        assert_eq!(summary, "Read file (3 lines, 17 chars)");
    }

    #[test]
    fn test_write_file() {
        let summary = summarize_tool_result("write_file", Some("wrote 100 bytes"), None);
        assert_eq!(summary, "File written successfully");
    }

    #[test]
    fn test_edit_file() {
        let summary = summarize_tool_result("edit_file", Some("patched"), None);
        assert_eq!(summary, "File edited successfully");
    }

    #[test]
    fn test_search_no_matches() {
        let summary = summarize_tool_result("search", Some("No matches found"), None);
        assert_eq!(summary, "Search completed (0 matches)");
    }

    #[test]
    fn test_search_with_matches() {
        let output =
            "src/main.rs:10: fn main()\nsrc/lib.rs:5: pub mod config\nsrc/app.rs:1: use std";
        let summary = summarize_tool_result("search", Some(output), None);
        assert_eq!(summary, "Search completed (3 matches found)");
    }

    #[test]
    fn test_list_files() {
        let output = "file1.rs\nfile2.rs\nfile3.rs";
        let summary = summarize_tool_result("list_files", Some(output), None);
        assert_eq!(summary, "Listed directory (3 items)");
    }

    #[test]
    fn test_bash_short_output() {
        let summary = summarize_tool_result("run_command", Some("hello world"), None);
        assert_eq!(summary, "Output: hello world");
    }

    #[test]
    fn test_bash_long_output() {
        let output = (0..20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let summary = summarize_tool_result("run_command", Some(&output), None);
        assert_eq!(summary, "Command executed (20 lines of output)");
    }

    #[test]
    fn test_web_fetch() {
        let summary = summarize_tool_result("web_fetch", Some("<html>...</html>"), None);
        assert_eq!(summary, "Content fetched successfully");
    }

    #[test]
    fn test_git_short() {
        let summary = summarize_tool_result("git", Some("Already up to date."), None);
        assert_eq!(summary, "Output: Already up to date.");
    }

    #[test]
    fn test_generic_short() {
        let summary = summarize_tool_result("unknown_tool", Some("done"), None);
        assert_eq!(summary, "done");
    }

    #[test]
    fn test_generic_long() {
        let output = "x".repeat(200);
        let summary = summarize_tool_result("unknown_tool", Some(&output), None);
        assert!(summary.contains("Success"));
        assert!(summary.contains("200 chars"));
    }
}
