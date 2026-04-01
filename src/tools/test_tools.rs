use crate::policy::CompiledPolicy;
use crate::tools::GatewayError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run an allowlisted test command (e.g. `cargo test`) with a constrained working directory.
pub fn run_tests(policy: &CompiledPolicy, args: &Value) -> Result<Value, GatewayError> {
    let cmd_path = args
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GatewayError::InvalidArgs("missing string 'command' (absolute path)".into()))?;
    let command = PathBuf::from(cmd_path);
    if !policy.command_allowed(&command) {
        return Err(GatewayError::CommandDenied(command));
    }

    let cwd = args
        .get("working_directory")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GatewayError::InvalidArgs("missing string 'working_directory'".into()))?;
    let cwd_path = PathBuf::from(cwd);
    if !policy.path_allowed_for_read(&cwd_path) {
        return Err(GatewayError::PathReadDenied(cwd_path));
    }

    let extra: Vec<String> = args
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut c = Command::new(&command);
    c.current_dir(&cwd_path);
    c.args(&extra);
    c.env_clear();
    c.env("PATH", std::env::var("PATH").unwrap_or_default());
    c.env("HOME", std::env::var("HOME").unwrap_or_default());
    c.env("RUSTUP_HOME", std::env::var("RUSTUP_HOME").unwrap_or_default());
    c.env("CARGO_HOME", std::env::var("CARGO_HOME").unwrap_or_default());

    let output = c.output()?;

    Ok(json!({
        "command": cmd_path,
        "working_directory": cwd,
        "args": extra,
        "status": output.status.code(),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
    }))
}

/// Narrow git surface: `git status --porcelain` only, explicit git binary path, requires `capabilities.git`.
pub fn git_status(policy: &CompiledPolicy, args: &Value) -> Result<Value, GatewayError> {
    if !policy.git_allowed {
        return Err(GatewayError::GitDenied);
    }
    let cmd_path = args
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GatewayError::InvalidArgs("missing string 'command' (path to git binary)".into()))?;
    let command = PathBuf::from(cmd_path);
    if !policy.command_allowed(&command) {
        return Err(GatewayError::CommandDenied(command));
    }

    let cwd = args
        .get("working_directory")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GatewayError::InvalidArgs("missing string 'working_directory'".into()))?;
    let cwd_path = Path::new(cwd);
    if !policy.path_allowed_for_read(cwd_path) {
        return Err(GatewayError::PathReadDenied(cwd_path.to_path_buf()));
    }

    let output = Command::new(&command)
        .current_dir(cwd_path)
        .args(["status", "--porcelain"])
        .output()?;

    Ok(json!({
        "working_directory": cwd,
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "status": output.status.code(),
    }))
}
