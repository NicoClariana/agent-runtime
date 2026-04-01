//! Native desktop UI (egui) to run agent workflows and inspect audit output.
//!
//! Build a release binary:
//!   cargo build --release --bin agent-runtime-gui
//! macOS app bundle (optional):
//!   ./packaging/macos/make-app-bundle.sh

use agent_runtime::manifest::AgentManifest;
use agent_runtime::policy::CompiledPolicy;
use agent_runtime::runner;
use eframe::egui;
use serde_json::Value as Json;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

/// Prefer Resources inside a macOS .app (bundled agents/tasks), then repo dir, then cwd.
fn project_root() -> PathBuf {
    if let Some(root) = bundled_resources_root() {
        return root;
    }
    if let Some(dir) = option_env!("CARGO_MANIFEST_DIR").map(PathBuf::from) {
        if dir.join("agents").is_dir() {
            return dir;
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if cwd.join("agents").is_dir() {
            return cwd;
        }
    }
    walk_up_for_agents().unwrap_or_else(|| PathBuf::from("."))
}

/// `App.app/Contents/MacOS/exe` → `App.app/Contents/Resources` if it contains `agents/`.
fn bundled_resources_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let resources = contents.join("Resources");
    if resources.join("agents").is_dir() {
        Some(resources)
    } else {
        None
    }
}

fn default_runs_dir(project_root: &Path) -> PathBuf {
    if bundled_resources_root().is_some() {
        dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("agent-runtime")
            .join("runs")
    } else {
        project_root.join("runs")
    }
}

fn walk_up_for_agents() -> Option<PathBuf> {
    let start = std::env::current_dir().ok()?;
    let mut p = start.clone();
    for _ in 0..12 {
        if p.join("agents").is_dir() {
            return Some(p);
        }
        if !p.pop() {
            break;
        }
    }
    None
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Agent Runtime")
            .with_inner_size([920.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Agent Runtime",
        native_options,
        Box::new(|cc| Ok(Box::new(RuntimeApp::new(cc)))),
    )
}

struct RuntimeApp {
    manifest_path: String,
    task_path: String,
    runs_dir: String,
    policy_preview: String,
    run_log: String,
    status: String,
    running: bool,
    worker_rx: Option<Receiver<WorkerResult>>,
}

enum WorkerResult {
    Finished(Result<String, String>),
}

impl RuntimeApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let root = project_root();
        let manifest = root.join("agents/log-watcher.yaml");
        let task = root.join("tasks/log-watcher.json");
        let manifest_path = manifest.to_string_lossy().into_owned();
        let task_path = task.to_string_lossy().into_owned();
        let runs_dir = default_runs_dir(&root).to_string_lossy().into_owned();

        let mut app = Self {
            manifest_path,
            task_path,
            runs_dir,
            policy_preview: String::new(),
            run_log: String::new(),
            status: "Pick a manifest and task, then click Run workflow.".into(),
            running: false,
            worker_rx: None,
        };
        app.refresh_policy_preview();
        app
    }

    fn refresh_policy_preview(&mut self) {
        let path = Path::new(&self.manifest_path);
        self.policy_preview = match AgentManifest::from_path(path) {
            Ok(m) => {
                let p = CompiledPolicy::compile(&m);
                format!(
                    "Agent: {}\n\
                     Role (informational only): {}\n\
                     Allowed tools: {:?}\n\
                     Read paths: {:?}\n\
                     Write paths: {:?}\n\
                     Allowed commands: {:?}\n\
                     network: {}  |  git: {}\n\
                     Limits: {}s runtime, {} MB, {} tool calls max",
                    p.agent_name,
                    m.role.mission,
                    p.allowed_tools.iter().collect::<Vec<_>>(),
                    p.read_paths,
                    p.write_paths,
                    p.allowed_commands,
                    p.network_allowed,
                    p.git_allowed,
                    p.max_runtime_seconds,
                    p.max_memory_mb,
                    p.max_tool_calls
                )
            }
            Err(e) => format!("Could not load manifest:\n{e}"),
        };
    }

    fn start_run(&mut self, ctx: egui::Context) {
        let manifest = PathBuf::from(&self.manifest_path);
        let task_path = PathBuf::from(&self.task_path);
        let runs_dir = PathBuf::from(&self.runs_dir);
        let (tx, rx): (Sender<WorkerResult>, Receiver<WorkerResult>) = mpsc::channel();
        self.worker_rx = Some(rx);
        self.running = true;
        self.status = "Running…".into();
        self.run_log.clear();

        std::thread::spawn(move || {
            let result = run_workflow(&manifest, &task_path, &runs_dir);
            let _ = tx.send(WorkerResult::Finished(result));
            ctx.request_repaint();
        });
    }

    fn poll_worker(&mut self) {
        let Some(rx) = &self.worker_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(WorkerResult::Finished(res)) => {
                self.running = false;
                self.worker_rx = None;
                match res {
                    Ok(s) => {
                        self.status = "Finished.".into();
                        self.run_log = s;
                    }
                    Err(e) => {
                        self.status = "Run failed.".into();
                        self.run_log = e;
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.running = false;
                self.worker_rx = None;
                self.status = "Worker stopped unexpectedly.".into();
            }
        }
    }
}

impl eframe::App for RuntimeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker();

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Agent Runtime");
                ui.label(egui::RichText::new("sandbox demo").weak());
            });
        });

        egui::SidePanel::left("help_panel")
            .resizable(true)
            .default_width(280.0)
            .show(ctx, |ui| {
                egui::CollapsingHeader::new("What is this?")
                    .default_open(true)
                    .show(ui, |ui| {
                    ui.label(
                        "This window is a native desktop app written in Rust. It is compiled \
                         to machine code on your Mac — the UI is not running inside a browser.",
                    );
                    ui.add_space(6.0);
                    ui.label(
                        "Cargo is Rust’s build tool (similar idea to npm or Gradle). Developers \
                         use `cargo build` to compile. You can open this app or the executable \
                         directly — you do not need to run Cargo yourself.",
                    );
                    ui.add_space(6.0);
                    ui.label(
                        "When you click Run workflow: the app loads a manifest (agent name, role \
                         text, and strict policy: tools, paths, commands), then runs a task (a list \
                         of tool steps). Each step is checked against policy; results and denials \
                         are saved under the runs folder (see path field) and shown on the right.",
                    );
                    if bundled_resources_root().is_some() {
                        ui.add_space(4.0);
                        ui.label(
                            "App bundle mode: default runs folder is under your user Caches \
                             (Library/Caches/agent-runtime/runs) so the app can write without \
                             modifying the .app.",
                        );
                    }
                });

                egui::CollapsingHeader::new("How the pieces fit together")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.label(
                            "Goal: define a safe “playing field” for an agent. The manifest is \
                             the contract: what it may touch and which narrow tools exist. The \
                             runtime checks every action against that contract.",
                        );
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("Manifest").strong());
                        ui.label(
                            "The rules of the game. It lists allowed tools, readable/writable \
                             paths, allowlisted commands, limits, and optional role text. Role \
                             text does not grant power — only the explicit lists do.",
                        );
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("Task").strong());
                        ui.label(
                            "A concrete list of steps (which tool, with what arguments). In this \
                             demo it is a fixed script. With a real LLM, the model would propose \
                             tool calls; this same runtime would still enforce the manifest before \
                             anything runs.",
                        );
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("Runs folder").strong());
                        ui.label(
                            "One subfolder per execution (named with a run ID). Each contains \
                             audit.jsonl (every decision), summary.json, a copy of the manifest, \
                             and workspace files. Think of it as the flight recorder.",
                        );
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("Sandbox level (today)").strong());
                        ui.label(
                            "Enforcement is inside this app: no raw OS access for log tools, and \
                             subprocesses only for explicitly allowlisted binaries. Stronger OS \
                             isolation (containers, seccomp, etc.) can wrap this same policy \
                             later without changing the manifest model.",
                        );
                    });

                egui::CollapsingHeader::new("Quick examples").show(ui, |ui| {
                    let root = project_root();
                    if ui.button("Log watcher (happy path)").clicked() {
                        self.manifest_path = root.join("agents/log-watcher.yaml").to_string_lossy().into();
                        self.task_path = root.join("tasks/log-watcher.json").to_string_lossy().into();
                        self.refresh_policy_preview();
                    }
                    if ui.button("Log watcher (denied read)").clicked() {
                        self.manifest_path = root.join("agents/log-watcher.yaml").to_string_lossy().into();
                        self.task_path = root.join("tasks/log-watcher-denied.json").to_string_lossy().into();
                        self.refresh_policy_preview();
                    }
                    if ui.button("Tester (allowlisted command)").clicked() {
                        self.manifest_path = root.join("agents/tester.yaml").to_string_lossy().into();
                        self.task_path = root.join("tasks/tester-ok.json").to_string_lossy().into();
                        self.refresh_policy_preview();
                    }
                    if ui.button("Tester (denied shell)").clicked() {
                        self.manifest_path = root.join("agents/tester.yaml").to_string_lossy().into();
                        self.task_path = root.join("tasks/tester-deny-cmd.json").to_string_lossy().into();
                        self.refresh_policy_preview();
                    }
                });

                ui.separator();
                ui.label(egui::RichText::new("Paths").strong());
                ui.label("Use Browse if you moved the project; examples assume the repo root.");
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Manifest:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.manifest_path)
                        .desired_width(f32::INFINITY),
                );
                if ui.button("Browse…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_file() {
                        self.manifest_path = p.to_string_lossy().into();
                        self.refresh_policy_preview();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Task:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.task_path).desired_width(f32::INFINITY),
                );
                if ui.button("Browse…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_file() {
                        self.task_path = p.to_string_lossy().into();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Runs dir:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.runs_dir).desired_width(240.0),
                );
                if ui.button("Refresh policy").clicked() {
                    self.refresh_policy_preview();
                }
                let run_clicked = ui
                    .add_enabled(!self.running, egui::Button::new("Run workflow"))
                    .clicked();
                if run_clicked {
                    self.start_run(ctx.clone());
                }
            });

            ui.label(egui::RichText::new(&self.status).strong());

            ui.separator();
            ui.label(egui::RichText::new("Compiled policy (from manifest)").strong());
            egui::ScrollArea::vertical()
                .id_salt("policy")
                .max_height(120.0)
                .show(ui, |ui| {
                    ui.monospace(&self.policy_preview);
                });

            ui.separator();
            ui.label(egui::RichText::new("Last run — readable summary").strong());
            ui.label(
                "Below: what mattered. Full JSON logs stay on disk in the run folder if you need them.",
            );
            egui::ScrollArea::vertical()
                .id_salt("runlog")
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.run_log)
                            .desired_width(f32::INFINITY)
                            .desired_rows(20)
                            .font(egui::TextStyle::Monospace),
                    );
                });
        });
    }
}

fn run_workflow(
    manifest: &Path,
    task_path: &Path,
    runs_dir: &Path,
) -> Result<String, String> {
    let m = AgentManifest::from_path(manifest).map_err(|e| e.to_string())?;
    let task = runner::load_task(task_path).map_err(|e| e.to_string())?;
    let outcome = runner::run(&m, manifest, &task, task_path, runs_dir).map_err(|e| e.to_string())?;

    let audit_path = outcome.run_dir.join("audit.jsonl");
    let audit = std::fs::read_to_string(&audit_path).unwrap_or_else(|e| format!("(no audit: {e})"));

    Ok(format_human_run_report(
        &outcome.run_id,
        &outcome.run_dir.display().to_string(),
        &outcome.exit_reason,
        outcome.tool_calls_used,
        &audit,
    ))
}

/// Turn audit JSONL into a short narrative; omit raw JSON dumps in the UI.
fn format_human_run_report(
    run_id: &str,
    run_dir: &str,
    exit_reason: &str,
    tool_calls_used: u32,
    audit_jsonl: &str,
) -> String {
    let mut out = String::new();
    out.push_str("══════════════════════════════════════\n");
    out.push_str(" WHAT HAPPENED\n");
    out.push_str("══════════════════════════════════════\n\n");
    out.push_str(&format!("Run ID:   {run_id}\n"));
    out.push_str(&format!("Saved in: {run_dir}\n"));
    out.push_str(&format!("Outcome:  {exit_reason}\n"));
    out.push_str(&format!("Tool calls recorded: {tool_calls_used}\n\n"));

    let events: Vec<Json> = audit_jsonl
        .lines()
        .filter_map(|line| serde_json::from_str::<Json>(line).ok())
        .filter_map(|row| row.get("event").cloned())
        .collect();

    let mut step_no = 0u32;
    for ev in &events {
        let Some(kind) = ev.get("kind").and_then(|k| k.as_str()) else {
            continue;
        };
        match kind {
            "run_start" => {
                if let Some(name) = ev.get("agent_name").and_then(|v| v.as_str()) {
                    out.push_str(&format!("Agent profile: {name}\n"));
                }
                out.push('\n');
                out.push_str("── Steps ──\n");
            }
            "tool_call" => {
                let allowed = ev.get("allowed").and_then(|v| v.as_bool()).unwrap_or(false);
                let tool = ev.get("tool").and_then(|v| v.as_str()).unwrap_or("?");
                let reason = ev
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty());
                step_no += 1;
                if allowed {
                    out.push_str(&format!("\n{step_no}. {tool} — allowed ✓\n"));
                } else {
                    out.push_str(&format!("\n{step_no}. {tool} — blocked ✗\n"));
                    if let Some(r) = reason {
                        out.push_str(&format!("   Reason: {r}\n"));
                    }
                }
            }
            "tool_result" => {
                let tool = ev.get("tool").and_then(|v| v.as_str()).unwrap_or("?");
                if let Some(err) = ev.get("error").and_then(|v| v.as_str()) {
                    out.push_str(&format!("   → Error: {err}\n"));
                } else if let Some(output) = ev.get("output") {
                    let summary = humanize_tool_output(tool, output);
                    for line in summary.lines() {
                        out.push_str("   ");
                        out.push_str(line);
                        out.push('\n');
                    }
                }
            }
            "run_end" => {}
            _ => {}
        }
    }

    out.push_str("\n══════════════════════════════════════\n");
    out.push_str(" ON DISK (full detail)\n");
    out.push_str("══════════════════════════════════════\n");
    out.push_str(&format!("{run_dir}/audit.jsonl  — every event, machine-readable\n"));
    out.push_str(&format!("{run_dir}/summary.json — compact step status\n"));

    out
}

fn humanize_tool_output(tool: &str, output: &Json) -> String {
    match tool {
        "list_logs" => {
            let dir = output
                .get("directory")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let entries = output
                .get("entries")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let names: Vec<&str> = output
                .get("entries")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|e| e.get("name").and_then(|n| n.as_str()))
                        .take(12)
                        .collect()
                })
                .unwrap_or_default();
            let mut s = format!("→ Listed {entries} item(s) in “{dir}”\n");
            if !names.is_empty() {
                s.push_str("→ Files/dirs: ");
                s.push_str(&names.join(", "));
                if entries > names.len() {
                    s.push_str(", …");
                }
            }
            s
        }
        "read_log" => {
            let path = output.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            let content = output.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let preview: String = content.chars().take(400).collect();
            let more = if content.len() > 400 { "\n… (truncated in UI)" } else { "" };
            format!("→ Read “{path}” ({len} chars)\n{preview}{more}", len = content.len())
        }
        "tail_log" => {
            let path = output.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            let lines = output
                .get("lines")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let n = lines.len();
            let mut s = format!("→ Last {n} line(s) from “{path}”:\n");
            for line in lines.iter().take(8) {
                if let Some(t) = line.as_str() {
                    s.push_str("   • ");
                    s.push_str(t);
                    s.push('\n');
                }
            }
            if n > 8 {
                s.push_str("   …\n");
            }
            s
        }
        "grep_log" => {
            let pat = output.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
            let matches = output
                .get("matches")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let mut s = format!("→ Pattern “{pat}”: {matches} match(es)\n");
            if let Some(arr) = output.get("matches").and_then(|v| v.as_array()) {
                for m in arr.iter().take(5) {
                    let file = m.get("file").and_then(|v| v.as_str());
                    let line = m.get("line").and_then(|v| v.as_u64());
                    let text = m.get("text").and_then(|v| v.as_str());
                    match (file, line, text) {
                        (Some(f), Some(ln), Some(t)) => {
                            s.push_str(&format!("   • {f}:{ln} — {t}\n"));
                        }
                        (_, Some(ln), Some(t)) => {
                            s.push_str(&format!("   • line {ln}: {t}\n"));
                        }
                        (_, _, Some(t)) => {
                            s.push_str(&format!("   • {t}\n"));
                        }
                        _ => {}
                    }
                }
                if matches > 5 {
                    s.push_str("   …\n");
                }
            }
            s
        }
        "run_tests" => {
            let cmd = output.get("command").and_then(|v| v.as_str()).unwrap_or("?");
            let status = output.get("status").and_then(|v| v.as_i64());
            let stderr = output
                .get("stderr")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .chars()
                .take(200)
                .collect::<String>();
            let mut s = format!("→ Command: {cmd}\n");
            if let Some(st) = status {
                s.push_str(&format!("→ Exit code: {st}\n"));
            }
            if !stderr.trim().is_empty() {
                s.push_str(&format!("→ Stderr (preview): {stderr}\n"));
            }
            s
        }
        "git_status" => {
            let out_txt = output
                .get("stdout")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if out_txt.is_empty() {
                "→ Git status: working tree clean (no porcelain lines)\n".into()
            } else {
                let preview: String = out_txt.chars().take(300).collect();
                format!("→ Git status (preview):\n{preview}\n")
            }
        }
        _ => {
            let compact = serde_json::to_string(output).unwrap_or_default();
            let short: String = compact.chars().take(200).collect();
            format!("→ {short}\n")
        }
    }
}
