use crate::policy::CompiledPolicy;
use crate::tools::GatewayError;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub fn list_logs(policy: &CompiledPolicy, args: &Value) -> Result<Value, GatewayError> {
    let dir = args
        .get("directory")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GatewayError::InvalidArgs("missing string 'directory'".into()))?;
    let path = PathBuf::from(dir);
    if !policy.path_allowed_for_read(&path) {
        return Err(GatewayError::PathReadDenied(path));
    }
    if !path.is_dir() {
        return Err(GatewayError::Other(format!("not a directory: {dir}")));
    }
    let mut entries = Vec::new();
    for e in fs::read_dir(&path)? {
        let e = e?;
        let meta = e.metadata()?;
        entries.push(json!({
            "name": e.file_name().to_string_lossy(),
            "is_file": meta.is_file(),
            "is_dir": meta.is_dir(),
        }));
    }
    Ok(json!({ "directory": dir, "entries": entries }))
}

const MAX_READ_BYTES: u64 = 512 * 1024;

pub fn read_log(policy: &CompiledPolicy, args: &Value) -> Result<Value, GatewayError> {
    let path_str = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GatewayError::InvalidArgs("missing string 'path'".into()))?;
    let path = PathBuf::from(path_str);
    if !policy.path_allowed_for_read(&path) {
        return Err(GatewayError::PathReadDenied(path));
    }
    if !path.is_file() {
        return Err(GatewayError::Other(format!("not a file: {path_str}")));
    }
    let len = fs::metadata(&path)?.len();
    if len > MAX_READ_BYTES {
        return Err(GatewayError::Other(format!(
            "file too large ({len} bytes); max {MAX_READ_BYTES}"
        )));
    }
    let content = fs::read_to_string(&path)?;
    Ok(json!({ "path": path_str, "content": content }))
}

pub fn tail_log(policy: &CompiledPolicy, args: &Value) -> Result<Value, GatewayError> {
    let path_str = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GatewayError::InvalidArgs("missing string 'path'".into()))?;
    let lines = args
        .get("lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(100)
        .min(10_000) as usize;
    let path = PathBuf::from(path_str);
    if !policy.path_allowed_for_read(&path) {
        return Err(GatewayError::PathReadDenied(path));
    }
    let text = fs::read_to_string(&path)?;
    let all_lines: Vec<&str> = text.lines().collect();
    let start = all_lines.len().saturating_sub(lines);
    let tail: Vec<&str> = all_lines[start..].to_vec();
    Ok(json!({
        "path": path_str,
        "lines": tail,
        "line_count": tail.len(),
    }))
}

pub fn grep_log(policy: &CompiledPolicy, args: &Value) -> Result<Value, GatewayError> {
    let path_str = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GatewayError::InvalidArgs("missing string 'path'".into()))?;
    let pattern = args
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GatewayError::InvalidArgs("missing string 'pattern'".into()))?;
    let path = PathBuf::from(path_str);
    if !policy.path_allowed_for_read(&path) {
        return Err(GatewayError::PathReadDenied(path.clone()));
    }

    if path.is_dir() {
        let mut matches = Vec::new();
        for e in walkdir_files(&path, policy)? {
            let text = match fs::read_to_string(&e) {
                Ok(t) => t,
                Err(_) => continue,
            };
            for (i, line) in text.lines().enumerate() {
                if line.contains(pattern) {
                    matches.push(json!({
                        "file": e.to_string_lossy(),
                        "line": i + 1,
                        "text": line,
                    }));
                }
            }
        }
        return Ok(json!({ "pattern": pattern, "matches": matches }));
    }

    if !path.is_file() {
        return Err(GatewayError::Other(format!("not a file or directory: {path_str}")));
    }
    let text = fs::read_to_string(&path)?;
    let mut matches = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.contains(pattern) {
            matches.push(json!({ "line": i + 1, "text": line }));
        }
    }
    Ok(json!({ "path": path_str, "pattern": pattern, "matches": matches }))
}

fn walkdir_files(root: &Path, policy: &CompiledPolicy) -> Result<Vec<PathBuf>, GatewayError> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        if !policy.path_allowed_for_read(&p) {
            continue;
        }
        for e in fs::read_dir(&p)? {
            let e = e?;
            let path = e.path();
            if e.file_type()?.is_dir() {
                stack.push(path);
            } else if e.file_type()?.is_file() && policy.path_allowed_for_read(&path) {
                out.push(path);
            }
        }
    }
    Ok(out)
}
