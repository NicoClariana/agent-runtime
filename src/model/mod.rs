pub mod mock;
pub mod types;

use thiserror::Error;
use types::{AgentDecision, ModelRequest};

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("model backend error: {0}")]
    Backend(String),
}

pub trait ModelClient: Send + Sync {
    fn decide(&self, req: &ModelRequest) -> Result<AgentDecision, ModelError>;
}
