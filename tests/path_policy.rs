//! Adversarial path and symlink cases for policy + gateway.

use agent_runtime::manifest::AgentManifest;
use agent_runtime::policy::CompiledPolicy;
use agent_runtime::tools::ToolGateway;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static CWD_LOCK: Mutex<()> = Mutex::new(());

struct CdGuard(PathBuf);
impl Drop for CdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

fn manifest_with_read_dir(dir: &Path) -> AgentManifest {
    let yaml = format!(
        r#"
name: path-test
role: {{ mission: "x" }}
capabilities:
  tools: [read_log, list_logs]
  filesystem:
    read: [{}]
    write: []
limits:
  max_runtime_seconds: 30
  max_memory_mb: 64
  max_tool_calls: 10
"#,
        dir.display()
    );
    AgentManifest::from_yaml_str(&yaml).unwrap()
}

#[test]
fn dotdot_escape_sibling_directory_denied() {
    let _lock = CWD_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let allowed = tmp.path().join("allowed");
    let secret = tmp.path().join("secret");
    fs::create_dir_all(allowed.join("nested")).unwrap();
    fs::create_dir_all(&secret).unwrap();
    fs::write(secret.join("flag.txt"), "nope").unwrap();

    let old = std::env::current_dir().unwrap();
    let _g = CdGuard(old);
    std::env::set_current_dir(tmp.path()).unwrap();

    let m = manifest_with_read_dir(&allowed);
    let p = CompiledPolicy::compile(&m);
    let gw = ToolGateway::new(p);

    let attack = Path::new("allowed/nested/../../secret/flag.txt");
    let denied = gw.invoke(
        "read_log",
        &serde_json::json!({ "path": attack.to_str().unwrap() }),
    );
    assert!(
        denied.is_err(),
        "expected traversal outside allowlist to be denied"
    );
}

#[cfg(unix)]
#[test]
fn symlink_inside_allowlist_to_outside_target_denied() {
    use std::os::unix::fs::symlink;

    let _lock = CWD_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let allowed = tmp.path().join("allowed");
    let secret = tmp.path().join("secret");
    fs::create_dir_all(&allowed).unwrap();
    fs::create_dir_all(&secret).unwrap();
    fs::write(secret.join("x.txt"), "exfil").unwrap();
    symlink(secret.join("x.txt"), allowed.join("via_link.txt")).unwrap();

    let old = std::env::current_dir().unwrap();
    let _g = CdGuard(old);
    std::env::set_current_dir(tmp.path()).unwrap();

    let m = manifest_with_read_dir(&allowed);
    let p = CompiledPolicy::compile(&m);
    let gw = ToolGateway::new(p);

    let path = allowed.join("via_link.txt");
    let denied = gw.invoke(
        "read_log",
        &serde_json::json!({ "path": path.to_str().unwrap() }),
    );
    assert!(
        denied.is_err(),
        "symlink target outside allowlist must not be readable: {denied:?}"
    );
}

#[test]
fn allowed_file_via_normalized_path_ok() {
    let _lock = CWD_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let allowed = tmp.path().join("allowed");
    fs::create_dir_all(allowed.join("nested")).unwrap();
    fs::write(allowed.join("nested/a.log"), "ok").unwrap();

    let old = std::env::current_dir().unwrap();
    let _g = CdGuard(old);
    std::env::set_current_dir(tmp.path()).unwrap();

    let m = manifest_with_read_dir(&allowed);
    let p = CompiledPolicy::compile(&m);
    let gw = ToolGateway::new(p);

    let ok = gw.invoke(
        "read_log",
        &serde_json::json!({ "path": "allowed/nested/../nested/a.log" }),
    );
    assert!(ok.is_ok(), "{ok:?}");
}

#[test]
fn empty_path_string_denied() {
    let tmp = tempfile::tempdir().unwrap();
    let allowed = tmp.path().join("a");
    fs::create_dir_all(&allowed).unwrap();
    let m = manifest_with_read_dir(&allowed);
    let p = CompiledPolicy::compile(&m);
    let gw = ToolGateway::new(p);
    let denied = gw.invoke("read_log", &serde_json::json!({ "path": "" }));
    assert!(denied.is_err());
}
