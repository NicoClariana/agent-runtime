//! End-to-end: policy + gateway + runner + audit.

use agent_runtime::manifest::AgentManifest;
use agent_runtime::policy::CompiledPolicy;
use agent_runtime::runner::{self, TaskFile, TaskStep};
use std::fs;
use tempfile::tempdir;

#[test]
fn log_watcher_denies_outside_allowlist() {
    let dir = tempdir().unwrap();
    let allowed = dir.path().join("allowed");
    fs::create_dir_all(&allowed).unwrap();
    fs::write(allowed.join("a.log"), "x").unwrap();
    let secret = dir.path().join("secret.log");
    fs::write(&secret, "x").unwrap();

    let yaml = format!(
        r#"
name: t
role: {{ mission: "x" }}
capabilities:
  tools: [read_log]
  filesystem:
    read: [{}]
    write: []
limits:
  max_runtime_seconds: 30
  max_memory_mb: 64
  max_tool_calls: 10
"#,
        allowed.display()
    );
    let m = AgentManifest::from_yaml_str(&yaml).unwrap();
    let policy = CompiledPolicy::compile(&m);
    let gw = agent_runtime::tools::ToolGateway::new(policy);

    let ok = gw.invoke(
        "read_log",
        &serde_json::json!({ "path": allowed.join("a.log").to_str().unwrap() }),
    );
    assert!(ok.is_ok());

    let denied = gw.invoke("read_log", &serde_json::json!({ "path": secret.to_str().unwrap() }));
    assert!(denied.is_err());
}

#[test]
fn runner_writes_audit_and_summary() {
    let base = tempdir().unwrap();
    let runs = base.path().join("runs");
    let log_dir = base.path().join("logs");
    fs::create_dir_all(&log_dir).unwrap();
    fs::write(log_dir.join("a.log"), "line\n").unwrap();

    let manifest_path = base.path().join("m.yaml");
    let yaml = format!(
        r#"
name: int
role: {{ mission: "test" }}
capabilities:
  tools: [tail_log]
  filesystem:
    read: [{}]
    write: []
limits:
  max_runtime_seconds: 30
  max_memory_mb: 64
  max_tool_calls: 10
"#,
        log_dir.display()
    );
    fs::write(&manifest_path, yaml).unwrap();

    let task_path = base.path().join("task.json");
    let task = TaskFile {
        description: "".into(),
        steps: vec![TaskStep {
            tool: "tail_log".into(),
            args: serde_json::json!({
                "path": log_dir.join("a.log").to_str().unwrap(),
                "lines": 5
            }),
        }],
    };
    fs::write(&task_path, serde_json::to_string(&task).unwrap()).unwrap();

    let m = AgentManifest::from_path(&manifest_path).unwrap();
    let outcome = runner::run(&m, &manifest_path, &task, &task_path, &runs).unwrap();
    assert!(outcome.run_dir.join("audit.jsonl").exists());
    assert!(outcome.run_dir.join("summary.json").exists());
    assert!(outcome.results[0].ok);
}
