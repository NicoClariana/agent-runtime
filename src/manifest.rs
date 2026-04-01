//! YAML/JSON agent manifest parsing.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentManifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub role: Role,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub output: OutputSpec,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Role {
    #[serde(default)]
    pub mission: String,
    #[serde(default)]
    pub non_goals: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Capabilities {
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub filesystem: FilesystemCaps,
    #[serde(default)]
    pub commands: CommandsCaps,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub git: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FilesystemCaps {
    #[serde(default)]
    pub read: Vec<PathBuf>,
    #[serde(default)]
    pub write: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CommandsCaps {
    #[serde(default)]
    pub allow: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Limits {
    #[serde(default = "default_max_runtime")]
    pub max_runtime_seconds: u64,
    #[serde(default = "default_max_memory_mb")]
    pub max_memory_mb: u64,
    #[serde(default = "default_max_tool_calls")]
    pub max_tool_calls: u32,
}

fn default_max_runtime() -> u64 {
    30
}

fn default_max_memory_mb() -> u64 {
    256
}

fn default_max_tool_calls() -> u32 {
    50
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OutputSpec {
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub schema: serde_json::Value,
}

impl AgentManifest {
    pub fn from_yaml_str(s: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(s)
    }

    pub fn from_path(path: &std::path::Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        if path.extension().is_some_and(|e| e == "json") {
            Ok(serde_json::from_str(&raw)?)
        } else {
            Ok(serde_yaml::from_str(&raw)?)
        }
    }
}
