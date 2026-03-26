//! write_todos tool — replace the entire todo list.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use opendev_runtime::{SubTodoItem, TodoManager, TodoStatus, parse_status, strip_markdown};
use opendev_tools_core::{BaseTool, ToolContext, ToolResult};

/// Tool that replaces the entire todo list.
#[derive(Debug)]
pub struct WriteTodosTool {
    manager: Arc<Mutex<TodoManager>>,
}

impl WriteTodosTool {
    pub fn new(manager: Arc<Mutex<TodoManager>>) -> Self {
        Self { manager }
    }
}

#[async_trait::async_trait]
impl BaseTool for WriteTodosTool {
    fn name(&self) -> &str {
        "write_todos"
    }

    fn description(&self) -> &str {
        "Replace the entire todo list with new items. Each item can be a string \
         or an object with content, status, and activeForm fields."
    }

    fn parameter_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "maxItems": 10,
                    "description": "List of parent todo items (max 10). Each can be a string or an object with 'content' (required), 'status' (optional), 'activeForm' (optional), and 'children' (optional array of sub-step strings, hidden in UI but shown in status output).",
                    "items": {
                        "oneOf": [
                            { "type": "string" },
                            {
                                "type": "object",
                                "properties": {
                                    "content": { "type": "string" },
                                    "status": { "type": "string" },
                                    "activeForm": { "type": "string" },
                                    "children": {
                                        "type": "array",
                                        "items": { "type": "string" },
                                        "description": "Sub-steps for this todo. Hidden in the user's UI but included in status output so you can track sub-steps."
                                    }
                                },
                                "required": ["content"]
                            }
                        ]
                    }
                }
            },
            "required": ["todos"]
        })
    }

    async fn execute(
        &self,
        args: HashMap<String, serde_json::Value>,
        _ctx: &ToolContext,
    ) -> ToolResult {
        let todos_val = match args.get("todos") {
            Some(v) if v.is_array() => v.as_array().unwrap(),
            _ => return ToolResult::fail("todos array is required"),
        };

        let mut items = Vec::new();
        for item in todos_val {
            if let Some(s) = item.as_str() {
                let title = strip_markdown(s);
                items.push((title, TodoStatus::Pending, String::new(), Vec::new()));
            } else if let Some(obj) = item.as_object() {
                let content = match obj.get("content").and_then(|v| v.as_str()) {
                    Some(c) => strip_markdown(c),
                    None => return ToolResult::fail("Each todo object requires a 'content' field"),
                };
                let status = obj
                    .get("status")
                    .and_then(|v| v.as_str())
                    .and_then(parse_status)
                    .unwrap_or(TodoStatus::Pending);
                let active_form = obj
                    .get("activeForm")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let children: Vec<SubTodoItem> = obj
                    .get("children")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| {
                                v.as_str().map(|s| SubTodoItem {
                                    title: strip_markdown(s),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                items.push((content, status, active_form, children));
            } else {
                return ToolResult::fail("Each todo must be a string or object");
            }
        }

        const MAX_TODOS: usize = 10;
        let was_truncated = items.len() > MAX_TODOS;
        if was_truncated {
            items.truncate(MAX_TODOS);
        }

        let mut mgr = match self.manager.lock() {
            Ok(m) => m,
            Err(e) => return ToolResult::fail(format!("Lock error: {e}")),
        };

        // Detect status-only updates: if the new titles match the existing
        // titles, just update statuses instead of clearing and recreating.
        // This avoids duplicate "Created N todos" display when the LLM
        // calls write_todos again with the same list.
        // Skip this optimization when any item has children (force full rewrite).
        let has_children = items.iter().any(|(_, _, _, c)| !c.is_empty());
        let existing_titles: Vec<String> = mgr.all().iter().map(|t| t.title.clone()).collect();
        let new_titles: Vec<&str> = items.iter().map(|(t, _, _, _)| t.as_str()).collect();
        let is_status_only = !has_children
            && !existing_titles.is_empty()
            && existing_titles.len() == new_titles.len()
            && existing_titles
                .iter()
                .zip(new_titles.iter())
                .all(|(a, b)| a == b);

        if is_status_only {
            // Collect (id, new_status) pairs first to avoid borrow conflict
            let updates: Vec<(usize, TodoStatus)> = mgr
                .all()
                .iter()
                .zip(items.iter())
                .filter(|(todo, (_, status, _, _))| todo.status != *status)
                .map(|(todo, (_, status, _, _))| (todo.id, *status))
                .collect();
            for (id, status) in &updates {
                mgr.set_status(*id, *status);
            }
            return if updates.is_empty() {
                ToolResult::ok("Todos unchanged. Now proceed with the next action.")
            } else {
                ToolResult::ok(format!(
                    "Updated {} todo status(es). Now proceed with the next action.\n\n{}",
                    updates.len(),
                    mgr.format_status_sorted()
                ))
            };
        }

        mgr.write_todos(items);
        let count = mgr.total();
        let truncation_note = if was_truncated {
            format!(" (truncated to {MAX_TODOS} — this is expected, do NOT call write_todos again)")
        } else {
            String::new()
        };
        ToolResult::ok(format!(
            "Created {count} todo(s){truncation_note}. Now proceed with the next action.\n\n{}",
            mgr.format_status_sorted()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool() -> (WriteTodosTool, Arc<Mutex<TodoManager>>) {
        let mgr = Arc::new(Mutex::new(TodoManager::new()));
        let tool = WriteTodosTool::new(Arc::clone(&mgr));
        (tool, mgr)
    }

    fn make_args(pairs: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[tokio::test]
    async fn test_write_todos_strings() {
        let (tool, mgr) = make_tool();
        let ctx = ToolContext::new("/tmp");
        let result = tool
            .execute(
                make_args(&[("todos", serde_json::json!(["Step A", "Step B", "Step C"]))]),
                &ctx,
            )
            .await;
        assert!(result.success);
        assert_eq!(mgr.lock().unwrap().total(), 3);
    }

    #[tokio::test]
    async fn test_write_todos_objects() {
        let (tool, mgr) = make_tool();
        let ctx = ToolContext::new("/tmp");
        let result = tool
            .execute(
                make_args(&[(
                    "todos",
                    serde_json::json!([
                        {"content": "First", "status": "in_progress", "activeForm": "Working on first"},
                        {"content": "Second"}
                    ]),
                )]),
                &ctx,
            )
            .await;
        assert!(result.success);
        let m = mgr.lock().unwrap();
        assert_eq!(m.total(), 2);
        assert_eq!(m.get(1).unwrap().status, TodoStatus::InProgress);
        assert_eq!(m.get(1).unwrap().active_form, "Working on first");
    }

    #[tokio::test]
    async fn test_write_todos_replaces() {
        let (tool, mgr) = make_tool();
        let ctx = ToolContext::new("/tmp");
        // Write initial
        tool.execute(make_args(&[("todos", serde_json::json!(["Old"]))]), &ctx)
            .await;
        assert_eq!(mgr.lock().unwrap().total(), 1);

        // Replace
        tool.execute(
            make_args(&[("todos", serde_json::json!(["New A", "New B"]))]),
            &ctx,
        )
        .await;
        assert_eq!(mgr.lock().unwrap().total(), 2);
        assert_eq!(mgr.lock().unwrap().get(1).unwrap().title, "New A");
    }

    #[tokio::test]
    async fn test_write_todos_missing_arg() {
        let (tool, _mgr) = make_tool();
        let ctx = ToolContext::new("/tmp");
        let result = tool.execute(HashMap::new(), &ctx).await;
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_write_todos_with_children() {
        let (tool, mgr) = make_tool();
        let ctx = ToolContext::new("/tmp");
        let result = tool
            .execute(
                make_args(&[(
                    "todos",
                    serde_json::json!([
                        {
                            "content": "Implement auth",
                            "activeForm": "Implementing auth",
                            "children": ["Add login endpoint", "Add token validation"]
                        },
                        {
                            "content": "Write tests",
                            "activeForm": "Writing tests",
                            "children": ["Unit tests", "Integration tests"]
                        }
                    ]),
                )]),
                &ctx,
            )
            .await;
        assert!(result.success);
        let m = mgr.lock().unwrap();
        // Only parent items counted
        assert_eq!(m.total(), 2);
        // Children stored on parents
        assert_eq!(m.get(1).unwrap().children.len(), 2);
        assert_eq!(m.get(1).unwrap().children[0].title, "Add login endpoint");
        assert_eq!(m.get(2).unwrap().children.len(), 2);
        // Children appear in result output
        let output = result.output.as_deref().unwrap_or("");
        assert!(output.contains("Add login endpoint"));
        assert!(output.contains("Integration tests"));
    }

    #[tokio::test]
    async fn test_write_todos_children_bypass_status_only() {
        let (tool, _mgr) = make_tool();
        let ctx = ToolContext::new("/tmp");
        // Write initial without children
        tool.execute(
            make_args(&[("todos", serde_json::json!(["Auth", "Tests"]))]),
            &ctx,
        )
        .await;

        // Write same titles but with children — should NOT use status-only path
        let result = tool
            .execute(
                make_args(&[(
                    "todos",
                    serde_json::json!([
                        {"content": "Auth", "children": ["Sub-step A"]},
                        {"content": "Tests", "children": ["Sub-step B"]}
                    ]),
                )]),
                &ctx,
            )
            .await;
        assert!(result.success);
        let output = result.output.as_deref().unwrap_or("");
        assert!(output.contains("Created 2 todo(s)"));
        assert!(output.contains("Sub-step A"));
    }
}
