use std::collections::HashMap;

use tokio::sync::mpsc;

/// Events emitted by a running subagent, consumed by the parent agent or TUI.
#[derive(Debug, Clone)]
pub enum SubagentEvent {
    /// Subagent started.
    Started {
        subagent_id: String,
        subagent_name: String,
        task: String,
        cancel_token: Option<tokio_util::sync::CancellationToken>,
    },
    /// Subagent made a tool call.
    ToolCall {
        subagent_id: String,
        subagent_name: String,
        tool_name: String,
        tool_id: String,
        args: HashMap<String, serde_json::Value>,
    },
    /// A subagent tool call completed.
    ToolComplete {
        subagent_id: String,
        subagent_name: String,
        tool_name: String,
        tool_id: String,
        success: bool,
    },
    /// Subagent finished.
    Finished {
        subagent_id: String,
        subagent_name: String,
        success: bool,
        result_summary: String,
        tool_call_count: usize,
        shallow_warning: Option<String>,
    },
    /// Token usage update from a subagent's LLM call.
    TokenUpdate {
        subagent_id: String,
        subagent_name: String,
        input_tokens: u64,
        output_tokens: u64,
    },
}

/// Progress callback that sends events through an mpsc channel.
///
/// Used to bridge subagent execution progress back to the TUI event loop.
pub struct ChannelProgressCallback {
    tx: mpsc::UnboundedSender<SubagentEvent>,
    /// Unique identifier for this subagent instance (disambiguates parallel subagents).
    subagent_id: String,
    /// Per-subagent cancellation token (child of parent's token).
    cancel_token: Option<tokio_util::sync::CancellationToken>,
}

impl ChannelProgressCallback {
    /// Create a new channel-based progress callback with a unique subagent ID.
    pub fn new(
        tx: mpsc::UnboundedSender<SubagentEvent>,
        subagent_id: String,
        cancel_token: Option<tokio_util::sync::CancellationToken>,
    ) -> Self {
        Self {
            tx,
            subagent_id,
            cancel_token,
        }
    }
}

impl std::fmt::Debug for ChannelProgressCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelProgressCallback").finish()
    }
}

impl opendev_agents::SubagentProgressCallback for ChannelProgressCallback {
    fn on_started(&self, subagent_name: &str, task: &str) {
        let _ = self.tx.send(SubagentEvent::Started {
            subagent_id: self.subagent_id.clone(),
            subagent_name: subagent_name.to_string(),
            task: task.to_string(),
            cancel_token: self.cancel_token.clone(),
        });
    }

    fn on_tool_call(
        &self,
        subagent_name: &str,
        tool_name: &str,
        tool_id: &str,
        args: &HashMap<String, serde_json::Value>,
    ) {
        let _ = self.tx.send(SubagentEvent::ToolCall {
            subagent_id: self.subagent_id.clone(),
            subagent_name: subagent_name.to_string(),
            tool_name: tool_name.to_string(),
            tool_id: tool_id.to_string(),
            args: args.clone(),
        });
    }

    fn on_tool_complete(&self, subagent_name: &str, tool_name: &str, tool_id: &str, success: bool) {
        let _ = self.tx.send(SubagentEvent::ToolComplete {
            subagent_id: self.subagent_id.clone(),
            subagent_name: subagent_name.to_string(),
            tool_name: tool_name.to_string(),
            tool_id: tool_id.to_string(),
            success,
        });
    }

    fn on_finished(&self, _subagent_name: &str, _success: bool, _result_summary: &str) {
        // Don't emit Finished here — SpawnSubagentTool::execute() sends the
        // authoritative Finished event with correct tool_call_count and shallow_warning.
    }

    fn on_token_usage(&self, subagent_name: &str, input_tokens: u64, output_tokens: u64) {
        let _ = self.tx.send(SubagentEvent::TokenUpdate {
            subagent_id: self.subagent_id.clone(),
            subagent_name: subagent_name.to_string(),
            input_tokens,
            output_tokens,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_subagent_event_variants() {
        let started = SubagentEvent::Started {
            subagent_id: "id-1".into(),
            subagent_name: "Explore".into(),
            task: "Find all TODO comments".into(),
            cancel_token: None,
        };
        assert!(matches!(started, SubagentEvent::Started { .. }));

        let finished = SubagentEvent::Finished {
            subagent_id: "id-1".into(),
            subagent_name: "Explore".into(),
            success: true,
            result_summary: "Found 5 TODOs".into(),
            tool_call_count: 3,
            shallow_warning: None,
        };
        assert!(matches!(finished, SubagentEvent::Finished { .. }));
    }

    #[tokio::test]
    async fn test_channel_progress_callback() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let cb = ChannelProgressCallback::new(tx, "test-id".into(), None);

        use opendev_agents::SubagentProgressCallback;
        cb.on_started("test-agent", "do a thing");
        cb.on_tool_call(
            "test-agent",
            "read_file",
            "tc-1",
            &std::collections::HashMap::new(),
        );
        cb.on_tool_complete("test-agent", "read_file", "tc-1", true);
        // on_finished is intentionally a no-op (SpawnSubagentTool sends the real Finished event)
        cb.on_finished("test-agent", true, "Done");

        let evt = rx.recv().await.unwrap();
        assert!(matches!(evt, SubagentEvent::Started { .. }));
        let evt = rx.recv().await.unwrap();
        assert!(matches!(evt, SubagentEvent::ToolCall { .. }));
        let evt = rx.recv().await.unwrap();
        assert!(matches!(evt, SubagentEvent::ToolComplete { .. }));
        // No Finished event expected — on_finished is a no-op
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_bridge_to_channel_end_to_end() {
        // Verify the full chain: SubagentEventBridge → ChannelProgressCallback → channel
        let (tx, mut rx) = mpsc::unbounded_channel();
        let subagent_id = "test-sa-id".to_string();
        let cb: Arc<dyn opendev_agents::SubagentProgressCallback> =
            Arc::new(ChannelProgressCallback::new(tx, subagent_id.clone(), None));

        // Create bridge (as SubagentManager::spawn would)
        let bridge = opendev_agents::SubagentEventBridge::new("Explorer".to_string(), cb);

        // Simulate react loop calling the bridge
        use opendev_agents::AgentEventCallback;
        let args = std::collections::HashMap::new();
        bridge.on_tool_started("tc-1", "read_file", &args);
        bridge.on_tool_finished("tc-1", true);
        bridge.on_token_usage(500, 100);

        // Verify events arrive on the channel
        let evt = rx.recv().await.unwrap();
        match evt {
            SubagentEvent::ToolCall {
                subagent_id: id,
                subagent_name,
                tool_name,
                tool_id,
                args: _,
            } => {
                assert_eq!(id, "test-sa-id");
                assert_eq!(subagent_name, "Explorer");
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_id, "tc-1");
            }
            other => panic!("Expected ToolCall, got {other:?}"),
        }

        let evt = rx.recv().await.unwrap();
        assert!(matches!(evt, SubagentEvent::ToolComplete { .. }));

        let evt = rx.recv().await.unwrap();
        match evt {
            SubagentEvent::TokenUpdate {
                input_tokens,
                output_tokens,
                ..
            } => {
                assert_eq!(input_tokens, 500);
                assert_eq!(output_tokens, 100);
            }
            other => panic!("Expected TokenUpdate, got {other:?}"),
        }
    }
}
