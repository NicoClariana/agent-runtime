//! Minimal desktop orchestrator UI.
use agent_runtime::agent;
use agent_runtime::manifest::AgentManifest;
use agent_runtime::model::mock::MockModelClient;
use agent_runtime::policy::CompiledPolicy;
use agent_runtime::runner;
use eframe::egui;
use eframe::egui::{Color32, RichText, Visuals};
use serde_json::Value as Json;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
type UiRunResult = Result<String, String>;

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
    PathBuf::from(".")
}

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

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Agent Orchestrator")
            .with_inner_size([1280.0, 880.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Agent Orchestrator",
        native_options,
        Box::new(|cc| Ok(Box::new(RuntimeApp::new(cc)))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunMode {
    ScriptedTask,
    AgentLoopMock,
}

impl RunMode {
    fn label(self) -> &'static str {
        match self {
            RunMode::ScriptedTask => "Scripted Task",
            RunMode::AgentLoopMock => "Agent Loop (Mock Brain)",
        }
    }
}

struct RuntimeApp {
    mode: RunMode,
    manifest_path: String,
    prompt_path: String,
    task_path: String,
    user_task: String,
    runs_dir: String,
    policy_preview: String,
    run_log: String,
    status: String,
    running: bool,
    theme_dark: bool,
    worker_rx: Option<Receiver<UiRunResult>>,
}

impl RuntimeApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let root = project_root();
        let mut s = Self {
            mode: RunMode::ScriptedTask,
            manifest_path: root
                .join("agents/log-watcher/manifest.yaml")
                .to_string_lossy()
                .into_owned(),
            prompt_path: root
                .join("agents/log-watcher/prompt.md")
                .to_string_lossy()
                .into_owned(),
            task_path: root.join("tasks/log-watcher.json").to_string_lossy().into_owned(),
            user_task: "Analyze logs for failures and summarize incidents.".into(),
            runs_dir: default_runs_dir(&root).to_string_lossy().into_owned(),
            policy_preview: String::new(),
            run_log: String::new(),
            status: "Ready".into(),
            running: false,
            theme_dark: true,
            worker_rx: None,
        };
        s.refresh_policy();
        s
    }

    fn refresh_policy(&mut self) {
        let path = Path::new(&self.manifest_path);
        self.policy_preview = match AgentManifest::from_path(path) {
            Ok(m) => {
                let p = CompiledPolicy::compile(&m);
                format!(
                    "Agent: {}\nRole: {}\nTools: {:?}\nRead: {:?}\nWrite: {:?}\nCommands: {:?}\nNetwork: {}\nGit: {}\nLimits: {}s / {}MB / {} calls",
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
                    p.max_tool_calls,
                )
            }
            Err(e) => format!("Manifest error: {e}"),
        };
    }

    fn start_run(&mut self, ctx: egui::Context) {
        let mode = self.mode;
        let manifest = PathBuf::from(self.manifest_path.clone());
        let prompt = PathBuf::from(self.prompt_path.clone());
        let task_path = PathBuf::from(self.task_path.clone());
        let user_task = self.user_task.clone();
        let runs = PathBuf::from(self.runs_dir.clone());
        let (tx, rx): (Sender<UiRunResult>, Receiver<UiRunResult>) = mpsc::channel();
        self.worker_rx = Some(rx);
        self.running = true;
        self.status = "Running".into();
        self.run_log.clear();

        std::thread::spawn(move || {
            let out = match mode {
                RunMode::ScriptedTask => run_scripted(&manifest, &task_path, &runs),
                RunMode::AgentLoopMock => run_mock_loop(&manifest, &prompt, &user_task, &runs),
            };
            let _ = tx.send(out);
            ctx.request_repaint();
        });
    }

    fn poll_worker(&mut self) {
        let Some(rx) = &self.worker_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.running = false;
                self.worker_rx = None;
                match result {
                    Ok(text) => {
                        self.status = "Finished".into();
                        self.run_log = text;
                    }
                    Err(err) => {
                        self.status = "Failed".into();
                        self.run_log = err;
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.running = false;
                self.worker_rx = None;
                self.status = "Worker disconnected".into();
            }
        }
    }
}

impl eframe::App for RuntimeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker();
        if self.theme_dark {
            ctx.set_visuals(Visuals::dark());
        } else {
            ctx.set_visuals(Visuals::light());
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12.0, 10.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Agent Orchestrator").color(Color32::from_rgb(120, 190, 255)));
                ui.separator();
                ui.label("Mode");
                egui::ComboBox::from_id_salt("mode")
                    .selected_text(self.mode.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.mode,
                            RunMode::ScriptedTask,
                            RunMode::ScriptedTask.label(),
                        );
                        ui.selectable_value(
                            &mut self.mode,
                            RunMode::AgentLoopMock,
                            RunMode::AgentLoopMock.label(),
                        );
                    });
                if ui.button("Refresh Policy").clicked() {
                    self.refresh_policy();
                }
                if ui
                    .add_enabled(!self.running, egui::Button::new("Run"))
                    .clicked()
                {
                    self.start_run(ctx.clone());
                }
                ui.separator();
                let status_color = match self.status.as_str() {
                    "Finished" => Color32::from_rgb(120, 220, 120),
                    "Failed" => Color32::from_rgb(255, 110, 110),
                    "Running" => Color32::from_rgb(255, 210, 120),
                    _ => Color32::from_rgb(170, 180, 200),
                };
                ui.label(RichText::new(format!("Status: {}", self.status)).color(status_color).strong());
                ui.separator();
                if ui.button(if self.theme_dark { "Dark" } else { "Light" }).clicked() {
                    self.theme_dark = !self.theme_dark;
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);

            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.heading("Configuration");
                ui.add_space(8.0);
                row_file(ui, "Manifest", &mut self.manifest_path, true, true);
                row_file(ui, "Prompt", &mut self.prompt_path, self.mode == RunMode::AgentLoopMock, true);
                row_file(ui, "Task", &mut self.task_path, self.mode == RunMode::ScriptedTask, true);
                row_file(ui, "Runs Dir", &mut self.runs_dir, true, false);
                if self.mode == RunMode::AgentLoopMock {
                    ui.label(RichText::new("User Task").strong());
                    ui.add(
                        egui::TextEdit::multiline(&mut self.user_task)
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    );
                }
            });

            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.heading("Policy Snapshot");
                    ui.separator();
                    if ui.button("Example: Log Watcher").clicked() {
                        let root = project_root();
                        self.manifest_path = root
                            .join("agents/log-watcher/manifest.yaml")
                            .to_string_lossy()
                            .into_owned();
                        self.prompt_path = root
                            .join("agents/log-watcher/prompt.md")
                            .to_string_lossy()
                            .into_owned();
                        self.task_path = root.join("tasks/log-watcher.json").to_string_lossy().into_owned();
                        self.user_task = "Analyze logs for failures and anomalies.".into();
                        self.refresh_policy();
                    }
                    if ui.button("Example: Tester").clicked() {
                        let root = project_root();
                        self.manifest_path = root
                            .join("agents/tester/manifest.yaml")
                            .to_string_lossy()
                            .into_owned();
                        self.prompt_path = root
                            .join("agents/tester/prompt.md")
                            .to_string_lossy()
                            .into_owned();
                        self.task_path = root.join("tasks/tester-ok.json").to_string_lossy().into_owned();
                        self.user_task = "Run approved tests and summarize failures.".into();
                        self.refresh_policy();
                    }
                });
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(190.0).show(ui, |ui| {
                    ui.monospace(&self.policy_preview);
                });
            });

            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.heading("Run Output");
                ui.add_space(6.0);
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(self.run_log.clone()).monospace().color(Color32::from_rgb(210, 220, 235)),
                            )
                            .wrap(),
                        );
                    });
            });
        });
    }
}

fn row_file(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    enabled: bool,
    file_dialog: bool,
) {
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
            if ui.button("Browse").clicked() {
                let picked = if file_dialog {
                    rfd::FileDialog::new().pick_file()
                } else {
                    rfd::FileDialog::new().pick_folder()
                };
                if let Some(path) = picked {
                    *value = path.to_string_lossy().into_owned();
                }
            }
        });
    });
}

fn run_scripted(manifest: &Path, task_path: &Path, runs_dir: &Path) -> Result<String, String> {
    let m = AgentManifest::from_path(manifest).map_err(|e| e.to_string())?;
    let task = runner::load_task(task_path).map_err(|e| e.to_string())?;
    let outcome = runner::run(&m, manifest, &task, task_path, runs_dir).map_err(|e| e.to_string())?;
    let audit_path = outcome.run_dir.join("audit.jsonl");
    let audit = std::fs::read_to_string(&audit_path).unwrap_or_default();
    Ok(format_human_run_report(
        "Scripted Task",
        &outcome.run_id,
        &outcome.run_dir.display().to_string(),
        &outcome.exit_reason,
        outcome.tool_calls_used,
        &audit,
        None,
    ))
}

fn run_mock_loop(
    manifest: &Path,
    prompt: &Path,
    user_task: &str,
    runs_dir: &Path,
) -> Result<String, String> {
    let model = MockModelClient;
    let outcome = agent::run_agent(manifest, prompt, user_task, runs_dir, &model)
        .map_err(|e| e.to_string())?;
    let audit_path = outcome.run_dir.join("audit.jsonl");
    let audit = std::fs::read_to_string(&audit_path).unwrap_or_default();
    Ok(format_human_run_report(
        "Agent Loop (Mock)",
        &outcome.run_id,
        &outcome.run_dir.display().to_string(),
        &outcome.exit_reason,
        outcome.tool_calls_used,
        &audit,
        outcome.final_answer.as_deref(),
    ))
}

fn format_human_run_report(
    run_type: &str,
    run_id: &str,
    run_dir: &str,
    exit_reason: &str,
    tool_calls_used: u32,
    audit_jsonl: &str,
    final_answer: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Run Type: {run_type}\nRun ID: {run_id}\nRun Dir: {run_dir}\nExit: {exit_reason}\nTool Calls: {tool_calls_used}\n\n"
    ));
    if let Some(ans) = final_answer {
        out.push_str("Final Answer:\n");
        out.push_str(ans);
        out.push_str("\n\n");
    }
    out.push_str("Steps:\n");
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
        if kind == "tool_call" {
            step_no += 1;
            let tool = ev.get("tool").and_then(|v| v.as_str()).unwrap_or("?");
            let allowed = ev.get("allowed").and_then(|v| v.as_bool()).unwrap_or(false);
            if allowed {
                out.push_str(&format!("  {step_no}. {tool}  [allowed]\n"));
            } else {
                let reason = ev
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("denied");
                out.push_str(&format!("  {step_no}. {tool}  [blocked: {reason}]\n"));
            }
        }
        if kind == "tool_result" {
            let tool = ev.get("tool").and_then(|v| v.as_str()).unwrap_or("?");
            if let Some(err) = ev.get("error").and_then(|v| v.as_str()) {
                out.push_str(&format!("       error: {err}\n"));
            } else if let Some(output) = ev.get("output") {
                let s = humanize_tool_output(tool, output);
                for line in s.lines() {
                    out.push_str("       ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
    }
    out.push_str("\nArtifacts:\n");
    out.push_str(&format!("  - {run_dir}/audit.jsonl\n"));
    out.push_str(&format!("  - {run_dir}/summary.json (scripted runs)\n"));
    out.push_str(&format!("  - {run_dir}/conversation.json (agent loop runs)\n"));
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
