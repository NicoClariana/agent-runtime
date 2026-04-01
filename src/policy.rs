//! Compile manifest into an authoritative policy object used at runtime.

use crate::manifest::AgentManifest;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Authoritative runtime policy. Role text does not grant capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledPolicy {
    pub agent_name: String,
    pub allowed_tools: HashSet<String>,
    pub read_paths: Vec<PathBuf>,
    pub write_paths: Vec<PathBuf>,
    pub allowed_commands: Vec<PathBuf>,
    pub network_allowed: bool,
    pub git_allowed: bool,
    pub max_runtime_seconds: u64,
    pub max_memory_mb: u64,
    pub max_tool_calls: u32,
}

impl CompiledPolicy {
    pub fn compile(manifest: &AgentManifest) -> Self {
        let allowed_tools: HashSet<String> =
            manifest.capabilities.tools.iter().cloned().collect();

        Self {
            agent_name: manifest.name.clone(),
            allowed_tools,
            read_paths: manifest.capabilities.filesystem.read.clone(),
            write_paths: manifest.capabilities.filesystem.write.clone(),
            allowed_commands: manifest.capabilities.commands.allow.clone(),
            network_allowed: manifest.capabilities.network,
            git_allowed: manifest.capabilities.git,
            max_runtime_seconds: manifest.limits.max_runtime_seconds,
            max_memory_mb: manifest.limits.max_memory_mb,
            max_tool_calls: manifest.limits.max_tool_calls,
        }
    }

    /// Resolve `path` for policy checks against configured prefixes (canonicalized when possible).
    pub fn path_allowed_for_read(&self, path: &Path) -> bool {
        self.path_under_any_prefix(path, &self.read_paths)
    }

    pub fn path_allowed_for_write(&self, path: &Path) -> bool {
        self.path_under_any_prefix(path, &self.write_paths)
    }

    fn path_under_any_prefix(&self, path: &Path, prefixes: &[PathBuf]) -> bool {
        let Ok(canonical) = path.canonicalize() else {
            return prefixes.iter().any(|p| path.starts_with(p));
        };
        prefixes.iter().any(|prefix| {
            if let Ok(can_prefix) = prefix.canonicalize() {
                canonical.starts_with(&can_prefix)
            } else {
                canonical.starts_with(prefix)
            }
        })
    }

    pub fn command_allowed(&self, cmd: &Path) -> bool {
        if self.allowed_commands.is_empty() {
            return false;
        }
        let Ok(canonical) = cmd.canonicalize() else {
            return self
                .allowed_commands
                .iter()
                .any(|allowed| cmd == allowed.as_path());
        };
        self.allowed_commands.iter().any(|allowed| {
            allowed
                .canonicalize()
                .map(|a| a == canonical)
                .unwrap_or_else(|_| allowed.as_path() == cmd)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::AgentManifest;

    #[test]
    fn compile_collects_tools() {
        let yaml = r#"
name: t
role: { mission: "x" }
capabilities:
  tools: [list_logs, read_log]
  filesystem:
    read: [/tmp/a]
    write: []
limits:
  max_runtime_seconds: 10
  max_memory_mb: 64
  max_tool_calls: 5
"#;
        let m = AgentManifest::from_yaml_str(yaml).unwrap();
        let p = CompiledPolicy::compile(&m);
        assert!(p.allowed_tools.contains("list_logs"));
        assert_eq!(p.max_tool_calls, 5);
    }
}
