//! CLI: `agent-runner launch|validate`

use agent_runtime::manifest::AgentManifest;
use agent_runtime::policy::CompiledPolicy;
use agent_runtime::runner;
use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "agent-runner")]
#[command(about = "Capability-based sandbox runtime for role-scoped AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate a manifest and print compiled policy summary
    Validate {
        #[arg(short, long)]
        manifest: PathBuf,
    },
    /// Run scripted task steps through the tool gateway (audit log under runs/)
    Launch {
        #[arg(short, long)]
        manifest: PathBuf,
        #[arg(short, long)]
        task: PathBuf,
        #[arg(short, long, default_value = "runs")]
        runs_dir: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Validate { manifest } => {
            let m = AgentManifest::from_path(&manifest)
                .with_context(|| format!("read manifest {}", manifest.display()))?;
            let p = CompiledPolicy::compile(&m);
            println!("Agent: {}", p.agent_name);
            println!("Allowed tools: {:?}", p.allowed_tools.iter().collect::<Vec<_>>());
            println!("Read paths: {:?}", p.read_paths);
            println!("Write paths: {:?}", p.write_paths);
            println!("Allowed commands: {:?}", p.allowed_commands);
            println!("network: {}", p.network_allowed);
            println!("git: {}", p.git_allowed);
            println!(
                "limits: {}s runtime, {} MB, {} tool calls",
                p.max_runtime_seconds, p.max_memory_mb, p.max_tool_calls
            );
            println!("Role (descriptive only): {}", m.role.mission);
        }
        Commands::Launch {
            manifest,
            task,
            runs_dir,
        } => {
            let m = AgentManifest::from_path(&manifest)?;
            let t = runner::load_task(&task)
                .with_context(|| format!("read task {}", task.display()))?;
            let outcome = runner::run(&m, &manifest, &t, &task, &runs_dir)?;
            println!("run_id: {}", outcome.run_id);
            println!("run_dir: {}", outcome.run_dir.display());
            println!("exit: {}", outcome.exit_reason);
            println!("tool_calls: {}", outcome.tool_calls_used);
        }
    }
    Ok(())
}
