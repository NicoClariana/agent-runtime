//! Executes a scripted task (tool steps) against the gateway — suitable for MVP demos without an LLM.

use crate::audit::{AuditEvent, AuditLog};
use crate::manifest::AgentManifest;
use crate::policy::CompiledPolicy;
use crate::sandbox::Sandbox;
use crate::tools::ToolGateway;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize, Serialize)]
pub struct TaskFile {
    #[serde(default)]
    pub description: String,
    pub steps: Vec<TaskStep>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TaskStep {
    pub tool: String,
    #[serde(default)]
    pub args: Value,
}

pub struct RunOutcome {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub exit_reason: String,
    pub tool_calls_used: u32,
    pub results: Vec<StepResult>,
}

#[derive(Debug)]
pub struct StepResult {
    pub tool: String,
    pub ok: bool,
    pub output: Option<Value>,
    pub error: Option<String>,
}

pub fn run(
    manifest: &AgentManifest,
    manifest_path: &Path,
    task: &TaskFile,
    task_path: &Path,
    runs_dir: &Path,
) -> anyhow::Result<RunOutcome> {
    let run_id = uuid::Uuid::new_v4().to_string();
    let policy = CompiledPolicy::compile(manifest);
    let gateway = ToolGateway::new(policy.clone());
    let sandbox = Sandbox::create(runs_dir, &run_id)?;
    let _copied = sandbox.copy_manifest(manifest_path);

    let mut audit = AuditLog::open(&sandbox.run_root, &run_id)?;
    audit.write_event(AuditEvent::RunStart {
        run_id: run_id.clone(),
        manifest_path: manifest_path.display().to_string(),
        task_path: task_path.display().to_string(),
        agent_name: manifest.name.clone(),
    })?;

    let deadline = Instant::now() + Duration::from_secs(policy.max_runtime_seconds);
    let mut tool_calls_used: u32 = 0;
    let mut results = Vec::new();

    for step in &task.steps {
        if Instant::now() > deadline {
            return write_end(
                &mut audit,
                &sandbox.run_root,
                &run_id,
                results,
                tool_calls_used,
                "deadline exceeded (max_runtime_seconds)",
            );
        }
        if tool_calls_used >= policy.max_tool_calls {
            return write_end(
                &mut audit,
                &sandbox.run_root,
                &run_id,
                results,
                tool_calls_used,
                "max_tool_calls exceeded",
            );
        }

        tool_calls_used += 1;
        let invoke = gateway.invoke(&step.tool, &step.args);
        let allowed = invoke.is_ok();

        let reason = invoke.as_ref().err().map(|e| e.to_string());
        audit.write_event(AuditEvent::ToolCall {
            run_id: run_id.clone(),
            tool: step.tool.clone(),
            args: step.args.clone(),
            allowed,
            reason,
        })?;

        let step_result = match invoke {
            Ok(out) => {
                audit.write_event(AuditEvent::ToolResult {
                    run_id: run_id.clone(),
                    tool: step.tool.clone(),
                    output: Some(out.clone()),
                    error: None,
                })?;
                StepResult {
                    tool: step.tool.clone(),
                    ok: true,
                    output: Some(out),
                    error: None,
                }
            }
            Err(e) => {
                let msg = e.to_string();
                audit.write_event(AuditEvent::ToolResult {
                    run_id: run_id.clone(),
                    tool: step.tool.clone(),
                    output: None,
                    error: Some(msg.clone()),
                })?;
                StepResult {
                    tool: step.tool.clone(),
                    ok: false,
                    output: None,
                    error: Some(msg),
                }
            }
        };
        results.push(step_result);
    }

    let exit_reason = "completed all steps".to_string();
    audit.write_event(AuditEvent::RunEnd {
        run_id: run_id.clone(),
        exit_reason: exit_reason.clone(),
        tool_calls_used,
    })?;

    write_summary(&sandbox.run_root, &run_id, &exit_reason, tool_calls_used, &results)?;

    Ok(RunOutcome {
        run_id,
        run_dir: sandbox.run_root,
        exit_reason,
        tool_calls_used,
        results,
    })
}

fn write_end(
    audit: &mut AuditLog,
    run_root: &Path,
    run_id: &str,
    results: Vec<StepResult>,
    tool_calls_used: u32,
    exit_reason: &str,
) -> anyhow::Result<RunOutcome> {
    audit.write_event(AuditEvent::RunEnd {
        run_id: run_id.to_string(),
        exit_reason: exit_reason.to_string(),
        tool_calls_used,
    })?;
    write_summary(run_root, run_id, exit_reason, tool_calls_used, &results)?;
    Ok(RunOutcome {
        run_id: run_id.to_string(),
        run_dir: run_root.to_path_buf(),
        exit_reason: exit_reason.to_string(),
        tool_calls_used,
        results,
    })
}

fn write_summary(
    run_root: &Path,
    run_id: &str,
    exit_reason: &str,
    tool_calls_used: u32,
    results: &[StepResult],
) -> anyhow::Result<()> {
    let summary_path = run_root.join("summary.json");
    let summary = serde_json::json!({
        "run_id": run_id,
        "exit_reason": exit_reason,
        "tool_calls_used": tool_calls_used,
        "steps": results.iter().map(|r| {
            serde_json::json!({
                "tool": r.tool,
                "ok": r.ok,
                "error": r.error,
            })
        }).collect::<Vec<_>>(),
    });
    std::fs::write(&summary_path, serde_json::to_string_pretty(&summary)?)?;
    Ok(())
}

pub fn load_task(path: &Path) -> anyhow::Result<TaskFile> {
    let raw = std::fs::read_to_string(path)?;
    if path.extension().is_some_and(|e| e == "yaml" || e == "yml") {
        Ok(serde_yaml::from_str(&raw)?)
    } else {
        Ok(serde_json::from_str(&raw)?)
    }
}
