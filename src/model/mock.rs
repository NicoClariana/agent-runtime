use crate::model::types::{AgentDecision, ConversationEvent, ModelRequest};
use crate::model::{ModelClient, ModelError};
use serde_json::json;

/// Deterministic backend for validating loop/tool routing without LLM randomness.
pub struct MockModelClient;

impl ModelClient for MockModelClient {
    fn decide(&self, req: &ModelRequest) -> Result<AgentDecision, ModelError> {
        let has_tool = |name: &str| req.tools.iter().any(|t| t.name == name);
        let called = |name: &str| {
            req.history.iter().any(|h| match h {
                ConversationEvent::ToolCall { tool, .. } => tool == name,
                _ => false,
            })
        };

        if has_tool("list_logs") && !called("list_logs") {
            return Ok(AgentDecision::ToolCall {
                tool: "list_logs".into(),
                args: json!({ "directory": "testdata/logs" }),
            });
        }
        if has_tool("tail_log") && !called("tail_log") {
            return Ok(AgentDecision::ToolCall {
                tool: "tail_log".into(),
                args: json!({ "path": "testdata/logs/app.log", "lines": 120 }),
            });
        }
        if has_tool("grep_log") && !called("grep_log") {
            return Ok(AgentDecision::ToolCall {
                tool: "grep_log".into(),
                args: json!({ "path": "testdata/logs/app.log", "pattern": "ERROR" }),
            });
        }

        let summary = format!(
            "Mock agent finished '{}' in {} step(s).",
            req.user_task,
            req.history
                .iter()
                .filter(|e| matches!(e, ConversationEvent::ToolCall { .. }))
                .count()
        );
        Ok(AgentDecision::Final { content: summary })
    }
}
