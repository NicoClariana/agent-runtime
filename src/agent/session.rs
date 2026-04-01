use crate::audit::{AuditEvent, AuditLog};
use crate::manifest::AgentManifest;
use crate::model::types::{AgentDecision, ConversationEvent, ModelRequest, ToolSchema};
use crate::model::ModelClient;
use crate::policy::CompiledPolicy;
use crate::sandbox::Sandbox;
use crate::tools::ToolGateway;
use anyhow::Context;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Serialize)]
pub struct AgentRunOutcome {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub exit_reason: String,
    pub tool_calls_used: u32,
    pub final_answer: Option<String>,
}

pub fn run_agent(
    manifest_path: &Path,
    prompt_path: &Path,
    user_task: &str,
    runs_dir: &Path,
    model: &dyn ModelClient,
) -> anyhow::Result<AgentRunOutcome> {
    let manifest = AgentManifest::from_path(manifest_path)
        .with_context(|| format!("read manifest {}", manifest_path.display()))?;
    let system_prompt = std::fs::read_to_string(prompt_path)
        .with_context(|| format!("read prompt {}", prompt_path.display()))?;

    let run_id = uuid::Uuid::new_v4().to_string();
    let policy = CompiledPolicy::compile(&manifest);
    let gateway = ToolGateway::new(policy.clone());
    let sandbox = Sandbox::create(runs_dir, &run_id)?;
    let _ = sandbox.copy_manifest(manifest_path);
    let _ = std::fs::copy(prompt_path, sandbox.run_root.join("prompt-used.md"));

    let mut audit = AuditLog::open(&sandbox.run_root, &run_id)?;
    audit.write_event(AuditEvent::RunStart {
        run_id: run_id.clone(),
        manifest_path: manifest_path.display().to_string(),
        task_path: format!("agent-task: {}", user_task),
        agent_name: manifest.name.clone(),
    })?;

    let tools = schemas_from_policy(&policy);
    let mut history = vec![ConversationEvent::UserTask {
        content: user_task.to_string(),
    }];

    let deadline = Instant::now() + Duration::from_secs(policy.max_runtime_seconds);
    let mut tool_calls_used = 0u32;
    let mut final_answer = None;
    let mut exit_reason = "completed".to_string();

    for _ in 0..policy.max_tool_calls {
        if Instant::now() > deadline {
            exit_reason = "deadline exceeded (max_runtime_seconds)".into();
            break;
        }
        let req = ModelRequest {
            system_prompt: system_prompt.clone(),
            user_task: user_task.to_string(),
            max_steps: policy.max_tool_calls,
            tools: tools.clone(),
            history: history.clone(),
        };
        let decision = model.decide(&req)?;

        match decision {
            AgentDecision::Final { content } => {
                final_answer = Some(content.clone());
                history.push(ConversationEvent::FinalAnswer { content });
                exit_reason = "model returned final answer".into();
                break;
            }
            AgentDecision::ToolCall { tool, args } => {
                tool_calls_used += 1;
                history.push(ConversationEvent::ToolCall {
                    tool: tool.clone(),
                    args: args.clone(),
                });

                let invoke = gateway.invoke(&tool, &args);
                let allowed = invoke.is_ok();
                let reason = invoke.as_ref().err().map(|e| e.to_string());
                audit.write_event(AuditEvent::ToolCall {
                    run_id: run_id.clone(),
                    tool: tool.clone(),
                    args: args.clone(),
                    allowed,
                    reason,
                })?;

                match invoke {
                    Ok(out) => {
                        history.push(ConversationEvent::ToolResult {
                            tool: tool.clone(),
                            result: out.clone(),
                        });
                        audit.write_event(AuditEvent::ToolResult {
                            run_id: run_id.clone(),
                            tool,
                            output: Some(out),
                            error: None,
                        })?;
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        history.push(ConversationEvent::ToolError {
                            tool: tool.clone(),
                            error: msg.clone(),
                        });
                        audit.write_event(AuditEvent::ToolResult {
                            run_id: run_id.clone(),
                            tool,
                            output: None,
                            error: Some(msg),
                        })?;
                    }
                }
            }
        }
    }

    if final_answer.is_none() && exit_reason == "completed" {
        exit_reason = "max_tool_calls reached without final answer".into();
    }

    audit.write_event(AuditEvent::RunEnd {
        run_id: run_id.clone(),
        exit_reason: exit_reason.clone(),
        tool_calls_used,
    })?;

    let trace = serde_json::json!({
        "run_id": run_id,
        "manifest": manifest_path.display().to_string(),
        "prompt": prompt_path.display().to_string(),
        "user_task": user_task,
        "tools": tools,
        "history": history,
        "final_answer": final_answer,
        "exit_reason": exit_reason,
    });
    std::fs::write(
        sandbox.run_root.join("conversation.json"),
        serde_json::to_string_pretty(&trace)?,
    )?;

    Ok(AgentRunOutcome {
        run_id,
        run_dir: sandbox.run_root,
        exit_reason,
        tool_calls_used,
        final_answer,
    })
}

fn schemas_from_policy(policy: &CompiledPolicy) -> Vec<ToolSchema> {
    let mut out = Vec::new();
    let mut names: Vec<_> = policy.allowed_tools.iter().cloned().collect();
    names.sort();
    for name in names {
        if let Some(schema) = schema_for_tool(&name) {
            out.push(schema);
        }
    }
    out
}

fn schema_for_tool(name: &str) -> Option<ToolSchema> {
    let (description, input_schema) = match name {
        "list_logs" => (
            "List files/dirs under an allowed directory",
            serde_json::json!({
                "type": "object",
                "properties": { "directory": { "type": "string" } },
                "required": ["directory"]
            }),
        ),
        "read_log" => (
            "Read a small allowed file as text",
            serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        ),
        "tail_log" => (
            "Read the last N lines from an allowed file",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "lines": { "type": "integer", "minimum": 1, "maximum": 10000 }
                },
                "required": ["path"]
            }),
        ),
        "grep_log" => (
            "Find plain-text pattern matches in an allowed file/dir",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "pattern": { "type": "string" }
                },
                "required": ["path", "pattern"]
            }),
        ),
        "run_tests" => (
            "Run an allowlisted executable in an allowed working directory",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "working_directory": { "type": "string" },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["command", "working_directory"]
            }),
        ),
        "git_status" => (
            "Run git status --porcelain using an allowlisted git binary",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "working_directory": { "type": "string" }
                },
                "required": ["command", "working_directory"]
            }),
        ),
        _ => return None,
    };
    Some(ToolSchema {
        name: name.to_string(),
        description: description.to_string(),
        input_schema,
    })
}
