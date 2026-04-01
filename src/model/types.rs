use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub system_prompt: String,
    pub user_task: String,
    pub max_steps: u32,
    pub tools: Vec<ToolSchema>,
    pub history: Vec<ConversationEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationEvent {
    UserTask { content: String },
    ToolCall { tool: String, args: Value },
    ToolResult { tool: String, result: Value },
    ToolError { tool: String, error: String },
    FinalAnswer { content: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentDecision {
    ToolCall { tool: String, args: Value },
    Final { content: String },
}
