//! Past sessions tool — browse and search historical conversation sessions.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use opendev_config::Paths;
use opendev_history::SessionManager;
use opendev_models::SessionMetadata;
use opendev_runtime::redact_secrets;
use opendev_tools_core::{BaseTool, ToolContext, ToolResult};

/// Tool for browsing and searching past conversation sessions.
///
/// Uses project-scoped session directories via `opendev_config::Paths`
/// and the `SessionManager` API from `opendev-history`.
#[derive(Debug)]
pub struct PastSessionsTool;

#[async_trait::async_trait]
impl BaseTool for PastSessionsTool {
    fn name(&self) -> &str {
        "past_sessions"
    }

    fn description(&self) -> &str {
        "Browse and search past conversation sessions for this project. \
         NOT for checking subagent status — subagent results arrive automatically."
    }

    fn parameter_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "read", "search", "info"],
                    "description": "Action to perform"
                },
                "session_id": {
                    "type": "string",
                    "description": "Session ID (for read/info)"
                },
                "query": {
                    "type": "string",
                    "description": "Search query (for search)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results (default: 20 list, 50 read, 10 search)"
                },
                "offset": {
                    "type": "integer",
                    "description": "Skip N items for pagination"
                },
                "include_archived": {
                    "type": "boolean",
                    "description": "Include archived sessions (default: false)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(
        &self,
        args: HashMap<String, serde_json::Value>,
        ctx: &ToolContext,
    ) -> ToolResult {
        // Guard: subagents cannot access past sessions
        if ctx.is_subagent {
            return ToolResult::fail(
                "past_sessions is not available to subagents. \
                 Focus on completing your assigned task.",
            );
        }

        let action = match args.get("action").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return ToolResult::fail("action is required"),
        };

        // Resolve project-scoped session directory
        let paths = Paths::new(None);
        let session_dir = paths.project_sessions_dir(&ctx.working_dir);

        // Guard: don't create directories as a side effect
        if !session_dir.exists() {
            return ToolResult::ok("No past sessions found for this project.".to_string());
        }

        // Construct SessionManager (dir already exists so create_dir_all is a no-op)
        let manager = match SessionManager::new(session_dir) {
            Ok(m) => m,
            Err(e) => return ToolResult::fail(format!("Failed to open session store: {e}")),
        };

        let current_session_id = ctx.session_id.as_deref();

        match action {
            "list" => action_list(&manager, &args, current_session_id),
            "read" => action_read(&manager, &args, current_session_id),
            "search" => action_search(&manager, &args),
            "info" => action_info(&manager, &args, current_session_id),
            _ => ToolResult::fail(format!(
                "Unknown action: {action}. Available: list, read, search, info"
            )),
        }
    }
}

/// Validate session_id: reject path traversal characters.
/// Returns `Some(ToolResult)` on failure, `None` on success.
fn validate_session_id(id: &str) -> Option<ToolResult> {
    if id.is_empty() || id.contains("..") || id.contains('/') || id.contains('\\') {
        Some(ToolResult::fail("Invalid session ID"))
    } else {
        None
    }
}

/// Guard: reject reads of the current session.
/// Returns `Some(ToolResult)` if blocked, `None` if allowed.
fn guard_current_session(session_id: &str, current: Option<&str>) -> Option<ToolResult> {
    if let Some(current_id) = current
        && session_id == current_id
    {
        Some(ToolResult::ok(
            "This is your current session — its messages are already in your context.".to_string(),
        ))
    } else {
        None
    }
}

fn format_timestamp(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M").to_string()
}

// --- Action implementations ---

fn action_list(
    manager: &SessionManager,
    args: &HashMap<String, serde_json::Value>,
    current_session_id: Option<&str>,
) -> ToolResult {
    let include_archived = args
        .get("include_archived")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
    let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

    let mut sessions: Vec<SessionMetadata> = manager.list_sessions(include_archived);

    // Exclude current session
    if let Some(current_id) = current_session_id {
        sessions.retain(|s| s.id != current_id);
    }

    // Sort by most recent first
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    if sessions.is_empty() {
        return ToolResult::ok("No past sessions found.".to_string());
    }

    let total = sessions.len();
    let page: Vec<&SessionMetadata> = sessions.iter().skip(offset).take(limit).collect();

    if page.is_empty() {
        return ToolResult::ok(format!("No sessions at offset {offset} (total: {total})."));
    }

    let mut output = format!(
        "Past sessions ({total} total, showing {}-{}):\n\n",
        offset + 1,
        offset + page.len(),
    );
    output.push_str(&format!(
        "{:<14} {:<40} {:>5} {:<17} {:>12}\n",
        "ID", "Title", "Msgs", "Updated", "Changes"
    ));
    output.push_str(&"-".repeat(90));
    output.push('\n');

    for meta in &page {
        let title = meta
            .title
            .as_deref()
            .unwrap_or("(untitled)")
            .chars()
            .take(38)
            .collect::<String>();
        let changes = format!("{}+/{}", meta.summary_additions, meta.summary_deletions);
        output.push_str(&format!(
            "{:<14} {:<40} {:>5} {:<17} {:>10}-\n",
            meta.id,
            title,
            meta.message_count,
            format_timestamp(&meta.updated_at),
            changes,
        ));
    }

    if total > offset + page.len() {
        output.push_str(&format!(
            "\nUse offset={} to see more.",
            offset + page.len()
        ));
    }

    let mut metadata = HashMap::new();
    metadata.insert("total".into(), serde_json::json!(total));
    ToolResult::ok_with_metadata(output, metadata)
}

fn action_read(
    manager: &SessionManager,
    args: &HashMap<String, serde_json::Value>,
    current_session_id: Option<&str>,
) -> ToolResult {
    let session_id = match args.get("session_id").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return ToolResult::fail("session_id is required for read"),
    };

    if let Some(r) = validate_session_id(session_id) {
        return r;
    }
    if let Some(r) = guard_current_session(session_id, current_session_id) {
        return r;
    }

    let session = match manager.load_session(session_id) {
        Ok(s) => s,
        Err(e) => return ToolResult::fail(format!("Session not found or corrupted: {e}")),
    };

    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
    let total_messages = session.messages.len();

    // Default offset: show the last `limit` messages
    let default_offset = total_messages.saturating_sub(limit);
    let offset = args
        .get("offset")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(default_offset);

    let page: Vec<_> = session.messages.iter().skip(offset).take(limit).collect();

    if page.is_empty() {
        return ToolResult::ok(format!(
            "Session {session_id}: no messages at offset {offset} (total: {total_messages})."
        ));
    }

    let title = session
        .metadata
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("(untitled)");

    let mut output = format!(
        "Session: {session_id} — \"{title}\"\n\
         Messages {}-{} of {total_messages}:\n\n",
        offset + 1,
        offset + page.len(),
    );

    for (i, msg) in page.iter().enumerate() {
        let idx = offset + i + 1;
        let role = &msg.role;
        // Truncate content to 500 chars
        let content = &msg.content;
        let truncated: String = if content.chars().count() > 500 {
            let s: String = content.chars().take(500).collect();
            format!("{s}...[truncated]")
        } else {
            content.to_string()
        };
        output.push_str(&format!("[{idx}] {role}:\n{truncated}\n\n"));
    }

    if offset + page.len() < total_messages {
        output.push_str(&format!(
            "Use offset={} to see more messages.",
            offset + page.len()
        ));
    }

    // Redact secrets from the entire output
    let redacted = redact_secrets(&output);

    let mut metadata = HashMap::new();
    metadata.insert("total_messages".into(), serde_json::json!(total_messages));
    ToolResult::ok_with_metadata(redacted, metadata)
}

fn action_search(
    manager: &SessionManager,
    args: &HashMap<String, serde_json::Value>,
) -> ToolResult {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) if !q.trim().is_empty() => q,
        _ => return ToolResult::fail("query is required and must be non-empty for search"),
    };

    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    let results = manager.search_sessions(query);

    if results.is_empty() {
        return ToolResult::ok(format!("No sessions matching \"{query}\"."));
    }

    let total = results.len();
    let shown = results.iter().take(limit);

    let mut output = format!("Search results for \"{query}\" ({total} sessions matched):\n\n");

    for (session_id, match_indices) in shown {
        let match_count = match_indices.len();

        // Try to load session for a context snippet
        let snippet = if let Ok(session) = manager.load_session(session_id) {
            if let Some(&first_idx) = match_indices.first() {
                if let Some(msg) = session.messages.get(first_idx) {
                    let content = &msg.content;
                    let preview: String = content.chars().take(100).collect();
                    format!("  {preview}...")
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        output.push_str(&format!("• {session_id} ({match_count} matches)\n"));
        if !snippet.is_empty() {
            output.push_str(&redact_secrets(&snippet));
            output.push('\n');
        }
        output.push('\n');
    }

    if total > limit {
        output.push_str(&format!("...and {} more sessions.", total - limit));
    }

    ToolResult::ok(output)
}

fn action_info(
    manager: &SessionManager,
    args: &HashMap<String, serde_json::Value>,
    current_session_id: Option<&str>,
) -> ToolResult {
    let session_id = match args.get("session_id").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return ToolResult::fail("session_id is required for info"),
    };

    if let Some(r) = validate_session_id(session_id) {
        return r;
    }
    if let Some(r) = guard_current_session(session_id, current_session_id) {
        return r;
    }

    let session = match manager.load_session(session_id) {
        Ok(s) => s,
        Err(e) => return ToolResult::fail(format!("Session not found or corrupted: {e}")),
    };

    let title = session
        .metadata
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("(untitled)");
    let working_dir = session.working_directory.as_deref().unwrap_or("(unknown)");
    let archived = if session.is_archived() { "yes" } else { "no" };
    let parent = session.parent_id.as_deref().unwrap_or("none");
    let subagent_count = session.subagent_sessions.len();
    let file_changes = session.file_changes.len();
    let summary = session.get_file_changes_summary();

    let output = format!(
        "Session: {session_id}\n\
         Title: {title}\n\
         Created: {}\n\
         Updated: {}\n\
         Messages: {}\n\
         Working directory: {working_dir}\n\
         File changes: {file_changes} (+{} lines, -{} lines across {} files)\n\
         Parent session: {parent}\n\
         Subagent sessions: {subagent_count}\n\
         Archived: {archived}",
        format_timestamp(&session.created_at),
        format_timestamp(&session.updated_at),
        session.messages.len(),
        summary.total_lines_added,
        summary.total_lines_removed,
        summary.total,
    );

    ToolResult::ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendev_models::{ChatMessage, Role};
    use tempfile::TempDir;

    fn make_message(role_str: &str, content: &str) -> ChatMessage {
        let role = match role_str {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "system" => Role::System,
            _ => Role::User,
        };
        ChatMessage {
            role,
            content: content.to_string(),
            timestamp: Utc::now(),
            metadata: HashMap::new(),
            tool_calls: Vec::new(),
            tokens: None,
            thinking_trace: None,
            reasoning_content: None,
            token_usage: None,
            provenance: None,
        }
    }

    /// Helper: create a SessionManager in a temp dir and add a session with messages.
    fn create_test_session(
        dir: &std::path::Path,
        id: &str,
        title: &str,
        messages: Vec<(&str, &str)>,
    ) {
        let mut manager = SessionManager::new(dir.to_path_buf()).unwrap();
        let session = manager.create_session();
        // Override the auto-generated ID
        let session_id = session.id.clone();

        // We need to manipulate the session directly
        if let Some(s) = manager.current_session_mut() {
            s.id = id.to_string();
            s.metadata.insert(
                "title".to_string(),
                serde_json::Value::String(title.to_string()),
            );
            for (role, content) in messages {
                s.messages.push(make_message(role, content));
            }
        }

        // Save with the correct ID — need to save via the manager
        if let Some(s) = manager.current_session() {
            manager.save_session(s).unwrap();
        }

        // Clean up the auto-generated session file if different from our ID
        if session_id != id {
            let _ = std::fs::remove_file(dir.join(format!("{session_id}.json")));
            let _ = std::fs::remove_file(dir.join(format!("{session_id}.jsonl")));
        }
    }

    #[test]
    fn test_list_empty() {
        let tmp = TempDir::new().unwrap();
        let manager = SessionManager::new(tmp.path().to_path_buf()).unwrap();
        let result = action_list(&manager, &HashMap::new(), None);
        assert!(result.success);
        assert!(result.output.unwrap().contains("No past sessions"));
    }

    #[test]
    fn test_list_sessions() {
        let tmp = TempDir::new().unwrap();
        create_test_session(
            tmp.path(),
            "sess-001",
            "First session",
            vec![("user", "hello"), ("assistant", "hi there")],
        );
        create_test_session(
            tmp.path(),
            "sess-002",
            "Second session",
            vec![("user", "how are you")],
        );

        let manager = SessionManager::new(tmp.path().to_path_buf()).unwrap();
        let result = action_list(&manager, &HashMap::new(), None);
        assert!(result.success);
        let out = result.output.unwrap();
        assert!(out.contains("sess-001") || out.contains("sess-002"));
    }

    #[test]
    fn test_list_excludes_current() {
        let tmp = TempDir::new().unwrap();
        create_test_session(
            tmp.path(),
            "current-sess",
            "Current",
            vec![("user", "test")],
        );
        create_test_session(tmp.path(), "other-sess", "Other", vec![("user", "test")]);

        let manager = SessionManager::new(tmp.path().to_path_buf()).unwrap();
        let result = action_list(&manager, &HashMap::new(), Some("current-sess"));
        assert!(result.success);
        let out = result.output.unwrap();
        assert!(!out.contains("current-sess"));
        assert!(out.contains("other-sess"));
    }

    #[test]
    fn test_read_pagination() {
        let tmp = TempDir::new().unwrap();
        let messages: Vec<(&str, &str)> = (0..10)
            .map(|i| {
                if i % 2 == 0 {
                    ("user", "question")
                } else {
                    ("assistant", "answer")
                }
            })
            .collect();
        create_test_session(tmp.path(), "paged-sess", "Paged", messages);

        let manager = SessionManager::new(tmp.path().to_path_buf()).unwrap();

        let mut args = HashMap::new();
        args.insert("session_id".to_string(), serde_json::json!("paged-sess"));
        args.insert("limit".to_string(), serde_json::json!(3));
        args.insert("offset".to_string(), serde_json::json!(0));

        let result = action_read(&manager, &args, None);
        assert!(result.success);
        let out = result.output.unwrap();
        assert!(out.contains("[1]"));
        assert!(out.contains("[3]"));
        // Should not contain message 4
        assert!(!out.contains("[4]"));
    }

    #[test]
    fn test_read_current_blocked() {
        let tmp = TempDir::new().unwrap();
        create_test_session(tmp.path(), "my-sess", "Mine", vec![("user", "hi")]);

        let manager = SessionManager::new(tmp.path().to_path_buf()).unwrap();

        let mut args = HashMap::new();
        args.insert("session_id".to_string(), serde_json::json!("my-sess"));

        let result = action_read(&manager, &args, Some("my-sess"));
        assert!(result.success); // It's an ok() with guidance, not a fail()
        assert!(result.output.unwrap().contains("current session"));
    }

    #[test]
    fn test_search() {
        let tmp = TempDir::new().unwrap();
        create_test_session(
            tmp.path(),
            "search-sess",
            "Searchable",
            vec![
                ("user", "How do I configure the database?"),
                ("assistant", "You can configure the database in config.yaml"),
            ],
        );

        let manager = SessionManager::new(tmp.path().to_path_buf()).unwrap();

        let mut args = HashMap::new();
        args.insert("query".to_string(), serde_json::json!("database"));

        let result = action_search(&manager, &args);
        assert!(result.success);
        let out = result.output.unwrap();
        assert!(out.contains("search-sess"));
    }

    #[test]
    fn test_info() {
        let tmp = TempDir::new().unwrap();
        create_test_session(
            tmp.path(),
            "info-sess",
            "Info Test",
            vec![("user", "hello"), ("assistant", "world")],
        );

        let manager = SessionManager::new(tmp.path().to_path_buf()).unwrap();

        let mut args = HashMap::new();
        args.insert("session_id".to_string(), serde_json::json!("info-sess"));

        let result = action_info(&manager, &args, None);
        assert!(result.success);
        let out = result.output.unwrap();
        assert!(out.contains("info-sess"));
        assert!(out.contains("Info Test"));
        assert!(out.contains("Messages: 2"));
    }

    #[test]
    fn test_path_traversal() {
        assert!(validate_session_id("../etc/passwd").is_some());
        assert!(validate_session_id("foo/bar").is_some());
        assert!(validate_session_id("foo\\bar").is_some());
        assert!(validate_session_id("").is_some());
        assert!(validate_session_id("valid-session-id").is_none());
    }

    #[tokio::test]
    async fn test_subagent_blocked() {
        let tool = PastSessionsTool;
        let mut args = HashMap::new();
        args.insert("action".to_string(), serde_json::json!("list"));

        let ctx = ToolContext::new("/tmp").with_subagent(true);

        let result = tool.execute(args, &ctx).await;
        assert!(!result.success);
        assert!(result.error.unwrap().contains("not available to subagents"));
    }

    #[test]
    fn test_secrets_redacted() {
        let tmp = TempDir::new().unwrap();
        create_test_session(
            tmp.path(),
            "secret-sess",
            "Secret Test",
            vec![(
                "user",
                "My API key is sk-ant-api03-abcdefghij1234567890abcdefghij1234567890abcdefghij",
            )],
        );

        let manager = SessionManager::new(tmp.path().to_path_buf()).unwrap();

        let mut args = HashMap::new();
        args.insert("session_id".to_string(), serde_json::json!("secret-sess"));

        let result = action_read(&manager, &args, None);
        assert!(result.success);
        let out = result.output.unwrap();
        assert!(!out.contains("abcdefghij1234567890"));
        assert!(out.contains("[REDACTED]"));
    }
}
