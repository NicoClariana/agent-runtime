//! Append-only audit log for runs.

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuditEvent {
    RunStart {
        run_id: String,
        manifest_path: String,
        task_path: String,
        agent_name: String,
    },
    ToolCall {
        run_id: String,
        tool: String,
        args: serde_json::Value,
        allowed: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    ToolResult {
        run_id: String,
        tool: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    RunEnd {
        run_id: String,
        exit_reason: String,
        tool_calls_used: u32,
    },
}

pub struct AuditLog {
    file: File,
    run_id: String,
}

impl AuditLog {
    pub fn open(run_dir: &Path, run_id: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(run_dir)?;
        let path = run_dir.join("audit.jsonl");
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file,
            run_id: run_id.to_string(),
        })
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn write_event(&mut self, event: AuditEvent) -> std::io::Result<()> {
        let ts = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let envelope = json!({
            "ts": ts,
            "event": serde_json::to_value(&event)?,
        });
        writeln!(self.file, "{}", serde_json::to_string(&envelope)?)?;
        self.file.flush()?;
        Ok(())
    }
}
