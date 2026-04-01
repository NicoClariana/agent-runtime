//! Capability-based sandbox runtime for role-scoped AI agents.
//!
//! Role is descriptive; policy is authoritative. All tool execution flows through
//! the gateway with explicit checks and audit records.

pub mod audit;
pub mod manifest;
pub mod policy;
pub mod runner;
pub mod sandbox;
pub mod tools;

pub use manifest::AgentManifest;
pub use policy::CompiledPolicy;
pub use runner::RunOutcome;
