//! Agent type definitions for specialized agent roles.
//!
//! Provides `AgentDefinition` for configuring different agent types (Code, Plan,
//! Test, Build) with distinct system prompts, thinking levels, tool sets, and
//! optional per-agent model overrides for thinking/critique phases.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use opendev_runtime::ThinkingLevel;

/// Predefined agent roles for common development tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentRole {
    /// General-purpose coding agent with full tool access.
    Code,
    /// Planning agent focused on architecture and task decomposition.
    Plan,
    /// Testing agent specialized in writing and running tests.
    Test,
    /// Build agent for compilation, linting, and CI tasks.
    Build,
}

impl AgentRole {
    /// Return the default system prompt snippet for this role.
    pub fn default_system_prompt(&self) -> &'static str {
        match self {
            AgentRole::Code => {
                "You are a coding agent. Your primary job is to read, write, and edit \
                 source code. Use tools to explore the codebase, make targeted edits, \
                 and verify your changes compile. Focus on correctness and minimal diffs."
            }
            AgentRole::Plan => {
                "You are a planning agent. Analyze the user's request and break it into \
                 concrete, ordered steps. Identify files to change, dependencies between \
                 tasks, and potential risks. Do NOT make code changes yourself — produce \
                 a structured plan for execution agents."
            }
            AgentRole::Test => {
                "You are a testing agent. Your job is to write, run, and verify tests. \
                 Read the relevant source code, write comprehensive tests covering edge \
                 cases, run them, and report results. Fix any failing tests you introduce."
            }
            AgentRole::Build => {
                "You are a build agent. Your job is to compile the project, run linters, \
                 and fix any build or lint errors. Focus on making the project build \
                 cleanly with zero warnings."
            }
        }
    }

    /// Return the default thinking level for this role.
    pub fn default_thinking_level(&self) -> ThinkingLevel {
        match self {
            AgentRole::Code => ThinkingLevel::Medium,
            AgentRole::Plan => ThinkingLevel::High,
            AgentRole::Test => ThinkingLevel::Low,
            AgentRole::Build => ThinkingLevel::Low,
        }
    }

    /// Return the default tool allowlist for this role.
    ///
    /// An empty vec means "all tools" (no restriction).
    pub fn default_tools(&self) -> Vec<String> {
        match self {
            AgentRole::Code => vec![], // all tools
            AgentRole::Plan => vec![
                "read_file".into(),
                "list_files".into(),
                "search".into(),
                "find_symbol".into(),
                "find_referencing_symbols".into(),
                "web_search".into(),
                "task_complete".into(),
            ],
            AgentRole::Test => vec![
                "read_file".into(),
                "write_file".into(),
                "edit_file".into(),
                "list_files".into(),
                "search".into(),
                "bash".into(),
                "task_complete".into(),
            ],
            AgentRole::Build => vec![
                "read_file".into(),
                "edit_file".into(),
                "bash".into(),
                "list_files".into(),
                "search".into(),
                "task_complete".into(),
            ],
        }
    }
}

impl std::fmt::Display for AgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentRole::Code => write!(f, "Code"),
            AgentRole::Plan => write!(f, "Plan"),
            AgentRole::Test => write!(f, "Test"),
            AgentRole::Build => write!(f, "Build"),
        }
    }
}

/// Full definition of an agent's configuration.
///
/// Combines a role with customizable system prompt, thinking level,
/// tool access, and optional per-agent model overrides for the
/// thinking and critique phases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// The agent's role.
    pub role: AgentRole,
    /// Custom system prompt (overrides the role default if set).
    pub system_prompt: Option<String>,
    /// Thinking level (overrides the role default if set).
    pub thinking_level: Option<ThinkingLevel>,
    /// Allowed tool names. Empty means all tools are available.
    pub tools: Vec<String>,
    /// Optional model override for the thinking phase.
    /// When set, the react loop uses this model for thinking calls
    /// instead of the default thinking model.
    pub thinking_model: Option<String>,
    /// Optional model override for the critique phase.
    /// When set, the react loop uses this model for critique calls
    /// instead of the default critique model.
    pub critique_model: Option<String>,
}

impl AgentDefinition {
    /// Create a new agent definition from a role with all defaults.
    pub fn from_role(role: AgentRole) -> Self {
        Self {
            role,
            system_prompt: None,
            thinking_level: None,
            tools: role.default_tools(),
            thinking_model: None,
            critique_model: None,
        }
    }

    /// Get the effective system prompt (custom or role default).
    pub fn effective_system_prompt(&self) -> &str {
        self.system_prompt
            .as_deref()
            .unwrap_or_else(|| self.role.default_system_prompt())
    }

    /// Get the effective thinking level (custom or role default).
    pub fn effective_thinking_level(&self) -> ThinkingLevel {
        self.thinking_level
            .unwrap_or_else(|| self.role.default_thinking_level())
    }

    /// Set a custom system prompt.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Set a custom thinking level.
    pub fn with_thinking_level(mut self, level: ThinkingLevel) -> Self {
        self.thinking_level = Some(level);
        self
    }

    /// Set the tool allowlist.
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.tools = tools;
        self
    }

    /// Set the thinking model override.
    pub fn with_thinking_model(mut self, model: impl Into<String>) -> Self {
        self.thinking_model = Some(model.into());
        self
    }

    /// Set the critique model override.
    pub fn with_critique_model(mut self, model: impl Into<String>) -> Self {
        self.critique_model = Some(model.into());
        self
    }

    /// Check if a tool is allowed for this agent.
    ///
    /// Returns `true` if the tool allowlist is empty (all tools allowed)
    /// or the tool name is in the allowlist.
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        self.tools.is_empty() || self.tools.iter().any(|t| t == tool_name)
    }

    /// Filter a set of tool schemas to only those allowed by this agent.
    pub fn filter_tool_schemas(&self, schemas: &[Value]) -> Vec<Value> {
        if self.tools.is_empty() {
            return schemas.to_vec();
        }
        schemas
            .iter()
            .filter(|schema| {
                let name = schema
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("");
                self.is_tool_allowed(name)
            })
            .cloned()
            .collect()
    }
}

/// Message sent when one agent hands off work to another.
///
/// Captures the outgoing agent's state so the receiving agent can
/// continue without re-discovering context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffMessage {
    /// Identifier of the agent handing off.
    pub from_agent: String,
    /// Identifier of the agent receiving the handoff.
    pub to_agent: String,
    /// High-level summary of what was accomplished.
    pub summary: String,
    /// Key findings discovered during execution.
    pub key_findings: Vec<String>,
    /// Actions that still need to be performed.
    pub pending_actions: Vec<String>,
}

impl HandoffMessage {
    /// Create a handoff from an agent definition and its conversation messages.
    ///
    /// Extracts the last assistant content as a summary and scans tool results
    /// for key findings. Any `task_complete` arguments contribute to pending
    /// actions when the status is not `"success"`.
    pub fn create_handoff(from: &AgentDefinition, to_role: &AgentRole, messages: &[Value]) -> Self {
        let from_name = from.role.to_string();
        let to_name = to_role.to_string();

        // Extract summary from the last assistant message
        let summary = messages
            .iter()
            .rev()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant"))
            .and_then(|m| m.get("content").and_then(|c| c.as_str()))
            .unwrap_or("No summary available")
            .to_string();

        // Collect key findings from tool results
        let mut key_findings = Vec::new();
        let mut pending_actions = Vec::new();

        for msg in messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if role == "tool" {
                let tool_name = msg.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");

                // Successful tool results are key findings (keep brief)
                if !content.starts_with("Error") && !content.is_empty() {
                    let finding = if content.len() > 200 {
                        format!("{tool_name}: {}...", &content[..200])
                    } else {
                        format!("{tool_name}: {content}")
                    };
                    key_findings.push(finding);
                }

                // Failed tools become pending actions
                if content.starts_with("Error") {
                    pending_actions.push(format!("Retry {tool_name}: {content}"));
                }
            }
        }

        // Cap findings to avoid bloat
        key_findings.truncate(10);
        pending_actions.truncate(10);

        HandoffMessage {
            from_agent: from_name,
            to_agent: to_name,
            summary,
            key_findings,
            pending_actions,
        }
    }

    /// Convert the handoff into a user message suitable for injecting into
    /// the receiving agent's message history.
    pub fn to_context_message(&self) -> Value {
        let findings = if self.key_findings.is_empty() {
            "None".to_string()
        } else {
            self.key_findings
                .iter()
                .map(|f| format!("- {f}"))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let pending = if self.pending_actions.is_empty() {
            "None".to_string()
        } else {
            self.pending_actions
                .iter()
                .map(|a| format!("- {a}"))
                .collect::<Vec<_>>()
                .join("\n")
        };

        serde_json::json!({
            "role": "user",
            "content": format!(
                "[HANDOFF from {from} agent]\n\n\
                 ## Summary\n{summary}\n\n\
                 ## Key Findings\n{findings}\n\n\
                 ## Pending Actions\n{pending}",
                from = self.from_agent,
                summary = self.summary,
            )
        })
    }
}

/// Group tool calls by file-path dependency for parallel execution.
///
/// Tools targeting different files can run in parallel. Tools targeting the
/// same file, or tools without a file path, are placed sequentially.
///
/// Returns a vec of groups, where each group can be executed in parallel.
pub fn can_parallelize(calls: &[Value]) -> Vec<Vec<Value>> {
    if calls.len() <= 1 {
        return vec![calls.to_vec()];
    }

    // Extract file path from each tool call's arguments
    let mut groups: Vec<Vec<Value>> = Vec::new();
    let mut file_to_group: HashMap<String, usize> = HashMap::new();
    // Tools without a file path go into a sequential fallback
    let mut no_path_group: Vec<Value> = Vec::new();

    for tc in calls {
        let path = extract_file_path(tc);

        match path {
            Some(p) => {
                if let Some(&group_idx) = file_to_group.get(&p) {
                    // Same file — must be sequential with the earlier call,
                    // so place in the same group
                    groups[group_idx].push(tc.clone());
                } else {
                    let idx = groups.len();
                    file_to_group.insert(p, idx);
                    groups.push(vec![tc.clone()]);
                }
            }
            None => {
                // No file path — cannot safely parallelize
                no_path_group.push(tc.clone());
            }
        }
    }

    // If all calls have no path, return them as a single sequential group
    if groups.is_empty() {
        return vec![no_path_group];
    }

    // Append no-path calls as their own sequential group
    if !no_path_group.is_empty() {
        groups.push(no_path_group);
    }

    groups
}

/// Extract the file path from a tool call's arguments.
///
/// Checks common argument names: `path`, `file_path`, `file`.
fn extract_file_path(tool_call: &Value) -> Option<String> {
    let args_str = tool_call
        .get("function")
        .and_then(|f| f.get("arguments"))
        .and_then(|a| a.as_str())
        .unwrap_or("{}");

    let args: Value = serde_json::from_str(args_str).unwrap_or_default();

    for key in &["path", "file_path", "file"] {
        if let Some(p) = args.get(*key).and_then(|v| v.as_str()) {
            return Some(p.to_string());
        }
    }
    None
}

/// Partial result data preserved when an agent is interrupted mid-execution.
///
/// Instead of discarding everything on interrupt, this struct captures the
/// tool results collected so far and any partial assistant content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialResult {
    /// Tool results that were successfully collected before the interrupt.
    pub completed_tool_results: Vec<Value>,
    /// The last assistant content chunk (may be incomplete).
    pub last_assistant_content: Option<String>,
    /// The iteration number at which the interrupt occurred.
    pub interrupted_at_iteration: usize,
    /// Number of tool calls that were completed in the interrupted batch.
    pub completed_tool_count: usize,
    /// Total tool calls that were requested in the interrupted batch.
    pub total_tool_count: usize,
}

impl PartialResult {
    /// Create a new partial result from interrupted execution state.
    pub fn from_interrupted_state(
        messages: &[Value],
        assistant_content: Option<&str>,
        iteration: usize,
        completed: usize,
        total: usize,
    ) -> Self {
        // Collect tool results from messages (most recent tool messages)
        let completed_tool_results: Vec<Value> = messages
            .iter()
            .rev()
            .take_while(|m| m.get("role").and_then(|r| r.as_str()) == Some("tool"))
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        Self {
            completed_tool_results,
            last_assistant_content: assistant_content.map(|s| s.to_string()),
            interrupted_at_iteration: iteration,
            completed_tool_count: completed,
            total_tool_count: total,
        }
    }

    /// Produce a human-readable summary of the partial result.
    pub fn summary(&self) -> String {
        format!(
            "Interrupted at iteration {} ({}/{} tool calls completed). {} tool result(s) preserved.",
            self.interrupted_at_iteration,
            self.completed_tool_count,
            self.total_tool_count,
            self.completed_tool_results.len(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- AgentRole tests ---

    #[test]
    fn test_agent_role_display() {
        assert_eq!(AgentRole::Code.to_string(), "Code");
        assert_eq!(AgentRole::Plan.to_string(), "Plan");
        assert_eq!(AgentRole::Test.to_string(), "Test");
        assert_eq!(AgentRole::Build.to_string(), "Build");
    }

    #[test]
    fn test_agent_role_default_system_prompt() {
        let prompt = AgentRole::Code.default_system_prompt();
        assert!(prompt.contains("coding agent"));

        let prompt = AgentRole::Plan.default_system_prompt();
        assert!(prompt.contains("planning agent"));

        let prompt = AgentRole::Test.default_system_prompt();
        assert!(prompt.contains("testing agent"));

        let prompt = AgentRole::Build.default_system_prompt();
        assert!(prompt.contains("build agent"));
    }

    #[test]
    fn test_agent_role_default_thinking_level() {
        assert_eq!(
            AgentRole::Code.default_thinking_level(),
            ThinkingLevel::Medium
        );
        assert_eq!(
            AgentRole::Plan.default_thinking_level(),
            ThinkingLevel::High
        );
        assert_eq!(AgentRole::Test.default_thinking_level(), ThinkingLevel::Low);
        assert_eq!(
            AgentRole::Build.default_thinking_level(),
            ThinkingLevel::Low
        );
    }

    #[test]
    fn test_agent_role_default_tools() {
        // Code has all tools (empty vec)
        assert!(AgentRole::Code.default_tools().is_empty());
        // Plan has restricted tools
        let plan_tools = AgentRole::Plan.default_tools();
        assert!(plan_tools.contains(&"read_file".to_string()));
        assert!(!plan_tools.contains(&"bash".to_string()));
        // Test has bash
        let test_tools = AgentRole::Test.default_tools();
        assert!(test_tools.contains(&"bash".to_string()));
        // Build has bash
        let build_tools = AgentRole::Build.default_tools();
        assert!(build_tools.contains(&"bash".to_string()));
    }

    // --- AgentDefinition tests ---

    #[test]
    fn test_agent_definition_from_role() {
        let def = AgentDefinition::from_role(AgentRole::Code);
        assert_eq!(def.role, AgentRole::Code);
        assert!(def.system_prompt.is_none());
        assert!(def.thinking_level.is_none());
        assert!(def.thinking_model.is_none());
        assert!(def.critique_model.is_none());
    }

    #[test]
    fn test_agent_definition_effective_system_prompt() {
        let def = AgentDefinition::from_role(AgentRole::Code);
        assert!(def.effective_system_prompt().contains("coding agent"));

        let def = def.with_system_prompt("Custom prompt");
        assert_eq!(def.effective_system_prompt(), "Custom prompt");
    }

    #[test]
    fn test_agent_definition_effective_thinking_level() {
        let def = AgentDefinition::from_role(AgentRole::Plan);
        assert_eq!(def.effective_thinking_level(), ThinkingLevel::High);

        let def = def.with_thinking_level(ThinkingLevel::Off);
        assert_eq!(def.effective_thinking_level(), ThinkingLevel::Off);
    }

    #[test]
    fn test_agent_definition_with_models() {
        let def = AgentDefinition::from_role(AgentRole::Code)
            .with_thinking_model("gpt-4o")
            .with_critique_model("claude-3-haiku");
        assert_eq!(def.thinking_model.as_deref(), Some("gpt-4o"));
        assert_eq!(def.critique_model.as_deref(), Some("claude-3-haiku"));
    }

    #[test]
    fn test_agent_definition_is_tool_allowed() {
        // Code agent: all tools allowed
        let code = AgentDefinition::from_role(AgentRole::Code);
        assert!(code.is_tool_allowed("bash"));
        assert!(code.is_tool_allowed("anything"));

        // Plan agent: restricted
        let plan = AgentDefinition::from_role(AgentRole::Plan);
        assert!(plan.is_tool_allowed("read_file"));
        assert!(!plan.is_tool_allowed("bash"));
    }

    #[test]
    fn test_agent_definition_filter_tool_schemas() {
        let schemas = vec![
            serde_json::json!({"function": {"name": "read_file"}}),
            serde_json::json!({"function": {"name": "bash"}}),
            serde_json::json!({"function": {"name": "search"}}),
        ];

        // Code agent keeps all
        let code = AgentDefinition::from_role(AgentRole::Code);
        assert_eq!(code.filter_tool_schemas(&schemas).len(), 3);

        // Plan agent filters out bash
        let plan = AgentDefinition::from_role(AgentRole::Plan);
        let filtered = plan.filter_tool_schemas(&schemas);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|s| {
            let name = s["function"]["name"].as_str().unwrap();
            name != "bash"
        }));
    }

    #[test]
    fn test_agent_definition_with_tools() {
        let def = AgentDefinition::from_role(AgentRole::Code)
            .with_tools(vec!["read_file".into(), "bash".into()]);
        assert!(def.is_tool_allowed("read_file"));
        assert!(def.is_tool_allowed("bash"));
        assert!(!def.is_tool_allowed("write_file"));
    }

    // --- HandoffMessage tests ---

    #[test]
    fn test_handoff_create() {
        let from = AgentDefinition::from_role(AgentRole::Code);
        let messages = vec![
            serde_json::json!({"role": "user", "content": "implement feature X"}),
            serde_json::json!({"role": "assistant", "content": "I read the file and found..."}),
            serde_json::json!({
                "role": "tool",
                "name": "read_file",
                "content": "fn main() { println!(\"hello\"); }",
                "tool_call_id": "tc-1"
            }),
            serde_json::json!({"role": "assistant", "content": "Done with initial analysis."}),
        ];

        let handoff = HandoffMessage::create_handoff(&from, &AgentRole::Test, &messages);
        assert_eq!(handoff.from_agent, "Code");
        assert_eq!(handoff.to_agent, "Test");
        assert_eq!(handoff.summary, "Done with initial analysis.");
        assert!(!handoff.key_findings.is_empty());
        assert!(handoff.pending_actions.is_empty());
    }

    #[test]
    fn test_handoff_with_errors() {
        let from = AgentDefinition::from_role(AgentRole::Build);
        let messages = vec![
            serde_json::json!({"role": "assistant", "content": "Trying to fix..."}),
            serde_json::json!({
                "role": "tool",
                "name": "bash",
                "content": "Error in bash: compilation failed",
                "tool_call_id": "tc-1"
            }),
        ];

        let handoff = HandoffMessage::create_handoff(&from, &AgentRole::Code, &messages);
        assert!(!handoff.pending_actions.is_empty());
        assert!(handoff.pending_actions[0].contains("Retry bash"));
    }

    #[test]
    fn test_handoff_empty_messages() {
        let from = AgentDefinition::from_role(AgentRole::Code);
        let handoff = HandoffMessage::create_handoff(&from, &AgentRole::Plan, &[]);
        assert_eq!(handoff.summary, "No summary available");
        assert!(handoff.key_findings.is_empty());
        assert!(handoff.pending_actions.is_empty());
    }

    #[test]
    fn test_handoff_to_context_message() {
        let handoff = HandoffMessage {
            from_agent: "Code".into(),
            to_agent: "Test".into(),
            summary: "Implemented feature X".into(),
            key_findings: vec!["Found bug in parser".into()],
            pending_actions: vec!["Write unit tests".into()],
        };
        let msg = handoff.to_context_message();
        assert_eq!(msg["role"], "user");
        let content = msg["content"].as_str().unwrap();
        assert!(content.contains("[HANDOFF from Code agent]"));
        assert!(content.contains("Implemented feature X"));
        assert!(content.contains("Found bug in parser"));
        assert!(content.contains("Write unit tests"));
    }

    #[test]
    fn test_handoff_to_context_message_empty_findings() {
        let handoff = HandoffMessage {
            from_agent: "Plan".into(),
            to_agent: "Code".into(),
            summary: "Plan complete".into(),
            key_findings: vec![],
            pending_actions: vec![],
        };
        let msg = handoff.to_context_message();
        let content = msg["content"].as_str().unwrap();
        assert!(content.contains("None"));
    }

    // --- can_parallelize tests ---

    #[test]
    fn test_can_parallelize_single_call() {
        let calls = vec![serde_json::json!({
            "function": {"name": "read_file", "arguments": "{\"path\": \"a.rs\"}"}
        })];
        let groups = can_parallelize(&calls);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 1);
    }

    #[test]
    fn test_can_parallelize_different_files() {
        let calls = vec![
            serde_json::json!({
                "function": {"name": "read_file", "arguments": "{\"path\": \"a.rs\"}"}
            }),
            serde_json::json!({
                "function": {"name": "read_file", "arguments": "{\"path\": \"b.rs\"}"}
            }),
            serde_json::json!({
                "function": {"name": "edit_file", "arguments": "{\"path\": \"c.rs\"}"}
            }),
        ];
        let groups = can_parallelize(&calls);
        // Three different files -> three groups (each parallelizable)
        assert_eq!(groups.len(), 3);
        assert!(groups.iter().all(|g| g.len() == 1));
    }

    #[test]
    fn test_can_parallelize_same_file() {
        let calls = vec![
            serde_json::json!({
                "function": {"name": "read_file", "arguments": "{\"path\": \"a.rs\"}"}
            }),
            serde_json::json!({
                "function": {"name": "edit_file", "arguments": "{\"path\": \"a.rs\"}"}
            }),
        ];
        let groups = can_parallelize(&calls);
        // Same file -> single group (sequential)
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
    }

    #[test]
    fn test_can_parallelize_mixed() {
        let calls = vec![
            serde_json::json!({
                "function": {"name": "read_file", "arguments": "{\"path\": \"a.rs\"}"}
            }),
            serde_json::json!({
                "function": {"name": "edit_file", "arguments": "{\"path\": \"a.rs\"}"}
            }),
            serde_json::json!({
                "function": {"name": "read_file", "arguments": "{\"path\": \"b.rs\"}"}
            }),
        ];
        let groups = can_parallelize(&calls);
        // a.rs group (2 calls) + b.rs group (1 call)
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn test_can_parallelize_no_path() {
        let calls = vec![
            serde_json::json!({
                "function": {"name": "bash", "arguments": "{\"command\": \"cargo test\"}"}
            }),
            serde_json::json!({
                "function": {"name": "bash", "arguments": "{\"command\": \"cargo build\"}"}
            }),
        ];
        let groups = can_parallelize(&calls);
        // No file paths -> single sequential group
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
    }

    #[test]
    fn test_can_parallelize_mixed_path_and_no_path() {
        let calls = vec![
            serde_json::json!({
                "function": {"name": "read_file", "arguments": "{\"path\": \"a.rs\"}"}
            }),
            serde_json::json!({
                "function": {"name": "bash", "arguments": "{\"command\": \"ls\"}"}
            }),
        ];
        let groups = can_parallelize(&calls);
        // a.rs group + no-path group
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn test_can_parallelize_empty() {
        let groups = can_parallelize(&[]);
        assert_eq!(groups.len(), 1);
        assert!(groups[0].is_empty());
    }

    #[test]
    fn test_can_parallelize_file_path_key() {
        // Uses "file_path" instead of "path"
        let calls = vec![
            serde_json::json!({
                "function": {"name": "write_file", "arguments": "{\"file_path\": \"x.rs\"}"}
            }),
            serde_json::json!({
                "function": {"name": "write_file", "arguments": "{\"file_path\": \"y.rs\"}"}
            }),
        ];
        let groups = can_parallelize(&calls);
        assert_eq!(groups.len(), 2);
    }

    // --- PartialResult tests ---

    #[test]
    fn test_partial_result_from_interrupted_state() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "do stuff"}),
            serde_json::json!({
                "role": "assistant",
                "content": "I'll read the files.",
                "tool_calls": [{"id": "tc-1", "function": {"name": "read_file", "arguments": "{}"}}]
            }),
            serde_json::json!({
                "role": "tool",
                "name": "read_file",
                "content": "file contents",
                "tool_call_id": "tc-1"
            }),
            serde_json::json!({
                "role": "tool",
                "name": "search",
                "content": "search results",
                "tool_call_id": "tc-2"
            }),
        ];

        let partial = PartialResult::from_interrupted_state(
            &messages,
            Some("I was analyzing the code..."),
            3,
            2,
            5,
        );

        assert_eq!(partial.completed_tool_results.len(), 2);
        assert_eq!(
            partial.last_assistant_content.as_deref(),
            Some("I was analyzing the code...")
        );
        assert_eq!(partial.interrupted_at_iteration, 3);
        assert_eq!(partial.completed_tool_count, 2);
        assert_eq!(partial.total_tool_count, 5);
    }

    #[test]
    fn test_partial_result_summary() {
        let partial = PartialResult {
            completed_tool_results: vec![serde_json::json!({"role": "tool", "content": "ok"})],
            last_assistant_content: None,
            interrupted_at_iteration: 5,
            completed_tool_count: 1,
            total_tool_count: 3,
        };
        let summary = partial.summary();
        assert!(summary.contains("iteration 5"));
        assert!(summary.contains("1/3"));
        assert!(summary.contains("1 tool result(s) preserved"));
    }

    #[test]
    fn test_partial_result_empty() {
        let partial = PartialResult::from_interrupted_state(&[], None, 1, 0, 0);
        assert!(partial.completed_tool_results.is_empty());
        assert!(partial.last_assistant_content.is_none());
        assert_eq!(
            partial.summary(),
            "Interrupted at iteration 1 (0/0 tool calls completed). 0 tool result(s) preserved."
        );
    }

    // --- extract_file_path tests ---

    #[test]
    fn test_extract_file_path_with_path() {
        let tc = serde_json::json!({
            "function": {"name": "read_file", "arguments": "{\"path\": \"src/main.rs\"}"}
        });
        assert_eq!(extract_file_path(&tc), Some("src/main.rs".to_string()));
    }

    #[test]
    fn test_extract_file_path_with_file_path() {
        let tc = serde_json::json!({
            "function": {"name": "write_file", "arguments": "{\"file_path\": \"out.txt\"}"}
        });
        assert_eq!(extract_file_path(&tc), Some("out.txt".to_string()));
    }

    #[test]
    fn test_extract_file_path_with_file() {
        let tc = serde_json::json!({
            "function": {"name": "edit", "arguments": "{\"file\": \"lib.rs\"}"}
        });
        assert_eq!(extract_file_path(&tc), Some("lib.rs".to_string()));
    }

    #[test]
    fn test_extract_file_path_none() {
        let tc = serde_json::json!({
            "function": {"name": "bash", "arguments": "{\"command\": \"ls\"}"}
        });
        assert_eq!(extract_file_path(&tc), None);
    }

    // --- Serialization round-trip tests ---

    #[test]
    fn test_agent_definition_serialization() {
        let def = AgentDefinition::from_role(AgentRole::Test)
            .with_thinking_model("gpt-4o")
            .with_critique_model("claude-3-haiku");
        let json = serde_json::to_string(&def).unwrap();
        let roundtrip: AgentDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.role, AgentRole::Test);
        assert_eq!(roundtrip.thinking_model.as_deref(), Some("gpt-4o"));
        assert_eq!(roundtrip.critique_model.as_deref(), Some("claude-3-haiku"));
    }

    #[test]
    fn test_handoff_message_serialization() {
        let handoff = HandoffMessage {
            from_agent: "Code".into(),
            to_agent: "Test".into(),
            summary: "Done".into(),
            key_findings: vec!["found a bug".into()],
            pending_actions: vec!["write tests".into()],
        };
        let json = serde_json::to_string(&handoff).unwrap();
        let roundtrip: HandoffMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.from_agent, "Code");
        assert_eq!(roundtrip.key_findings.len(), 1);
    }

    #[test]
    fn test_partial_result_serialization() {
        let partial = PartialResult {
            completed_tool_results: vec![serde_json::json!({"role": "tool"})],
            last_assistant_content: Some("partial".into()),
            interrupted_at_iteration: 2,
            completed_tool_count: 1,
            total_tool_count: 3,
        };
        let json = serde_json::to_string(&partial).unwrap();
        let roundtrip: PartialResult = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.interrupted_at_iteration, 2);
        assert_eq!(roundtrip.last_assistant_content.as_deref(), Some("partial"));
    }
}
