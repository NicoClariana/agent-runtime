//! Tool gateway: structured operations only; every call validates against [`CompiledPolicy`](crate::policy::CompiledPolicy).

mod log_tools;
mod test_tools;

use crate::policy::CompiledPolicy;
use serde_json::Value;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("tool '{0}' is not allowed by policy")]
    ToolNotAllowed(String),
    #[error("path not allowed for read: {0}")]
    PathReadDenied(PathBuf),
    #[error("path not allowed for write: {0}")]
    PathWriteDenied(PathBuf),
    #[error("command not allowlisted: {0}")]
    CommandDenied(PathBuf),
    #[error("git operations are disabled for this agent")]
    GitDenied,
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub struct ToolGateway {
    policy: CompiledPolicy,
}

impl ToolGateway {
    pub fn new(policy: CompiledPolicy) -> Self {
        Self { policy }
    }

    /// Policy reference for tests / introspection.
    pub fn policy(&self) -> &CompiledPolicy {
        &self.policy
    }

    pub fn invoke(&self, name: &str, args: &Value) -> Result<Value, GatewayError> {
        if !self.policy.allowed_tools.contains(name) {
            return Err(GatewayError::ToolNotAllowed(name.to_string()));
        }

        match name {
            "list_logs" => log_tools::list_logs(&self.policy, args),
            "read_log" => log_tools::read_log(&self.policy, args),
            "tail_log" => log_tools::tail_log(&self.policy, args),
            "grep_log" => log_tools::grep_log(&self.policy, args),
            "run_tests" => test_tools::run_tests(&self.policy, args),
            "git_status" => test_tools::git_status(&self.policy, args),
            _ => Err(GatewayError::ToolNotAllowed(name.to_string())),
        }
    }
}

