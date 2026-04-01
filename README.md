# Agent Runtime

Agent Runtime is a capability-based runtime demo for **role-scoped** agents. It does **not** embed an LLM. It shows how to:

1. **Load an agent manifest** — human-readable profile plus explicit permissions (tools, paths, commands, limits).
2. **Compile policy** — turn that manifest into an internal `CompiledPolicy` used for every decision. Role text is descriptive only; it does not grant capabilities.
3. **Run a scripted task** — a JSON/YAML file listing tool steps (`tool` + `args`). Each step goes through a tool gateway that checks policy before doing I/O or spawning a process.
4. **Record runs** — each execution gets a UUID, a folder under `runs/` (or the GUI’s cache path when bundled), with `audit.jsonl` (append-only events), `summary.json`, workspace dirs, and a copy of the manifest used.

## Binaries

| Binary | Purpose |
|--------|---------|
| **agent-runner** | CLI: `validate` (print compiled policy) and `launch` (run manifest + task). |
| **agent-runtime-gui** | Desktop UI (egui): same engine, human-readable run summary, quick examples, file pickers. |

## Built-in tools

Tools must appear in `capabilities.tools` to be invocable.

| Path | Tools |
|------|--------|
| **Log** (no shell) | `list_logs`, `read_log`, `tail_log`, `grep_log` |
| **Tester** | `run_tests` (only allowlisted absolute command, `working_directory` under read policy) |
| **Optional git** | `git_status` (only if `capabilities.git: true` and git binary allowlisted) |

## Sandbox stance (MVP)

Enforcement is in this process: structured tools + allowlisted subprocesses. It is **not** a full OS/container sandbox yet; the manifest model is meant to stay valid if you add containers/seccomp later.

## How the manifest is read

**Entry point:** `AgentManifest::from_path(path)` in `src/manifest.rs`.

1. Read the file as UTF-8 text.
2. Choose parser by file extension:
   - Extension **`.json`** → `serde_json::from_str`
   - Anything else (including `.yaml` / `.yml`) → `serde_yaml::from_str`
3. There is no schema merge from multiple files: **one file = one manifest**.

### Deserialized shape (Serde structs)

| Section | Rust type | Purpose |
|---------|-----------|---------|
| `name`, `description` | `String` | Agent identity / blurb |
| `role` | `Role { mission, non_goals }` | Informational mission and non-goals |
| `capabilities` | `Capabilities` | Authoritative caps |
| `capabilities.tools` | `Vec<String>` | Tool names the gateway may dispatch |
| `capabilities.filesystem.read` / `write` | `Vec<PathBuf>` | Path prefixes for read/write checks |
| `capabilities.commands.allow` | `Vec<PathBuf>` | Exact allowlisted executables for `run_tests` / `git_status` |
| `capabilities.network`, `capabilities.git` | `bool` | Flags (`git` enforced for `git_status`; `network` is mostly policy metadata in MVP) |
| `limits` | `max_runtime_seconds`, `max_memory_mb`, `max_tool_calls` | Runtime limits (runner enforces time + tool-call count; memory is not forced at OS level in MVP) |
| `output` | `format`, `schema` | Declarative output hints (not heavily used by runner yet) |

After load: `CompiledPolicy::compile(&manifest)` in `src/policy.rs` copies those fields into a `HashSet` of tools plus path/command lists. The gateway uses **`CompiledPolicy`**, not raw role strings, for allow/deny.

**Also available:** `AgentManifest::from_yaml_str(&str)` for tests and embedded YAML.

## Task files

Tasks are loaded by `runner::load_task(path)`: read file, then JSON or YAML (if extension is `yaml` / `yml`) into:

- **`TaskFile`:** optional `description`, `steps`: list of `TaskStep`
- **`TaskStep`:** `tool: String`, `args: serde_json::Value` (defaults to empty object)

The runner walks `steps` in order and calls `ToolGateway::invoke` for each.

### Shipped example tasks (`tasks/`)

| File | Manifest pairing | Intent |
|------|------------------|--------|
| `log-watcher.json` | `agents/log-watcher.yaml` | Happy path: `list_logs` → `tail_log` → `grep_log` on `testdata/logs` |
| `log-watcher-denied.json` | `agents/log-watcher.yaml` | Policy denial: `read_log` on `/etc/hosts` (outside read allowlist) |
| `tester-ok.json` | `agents/tester.yaml` | Allowlisted `/bin/true` in `testdata/sample-crate` |
| `tester-deny-cmd.json` | `agents/tester.yaml` | Denied: `run_tests` with `/bin/sh` (not on command allowlist) |
| `tester-git.json` | `agents/tester.yaml` | `git_status` with `/usr/bin/git` in sample repo (needs `git: true` + git in manifest) |

Paths in these JSON files are **relative to the process current working directory** (typically `agent-runtime/` when using CLI/GUI from that crate).

## Tests

Crate **agent-runtime** (run with `cargo test` from `agent-runtime/`):

| Test | Location | What it checks |
|------|----------|----------------|
| `policy::tests::compile_collects_tools` | `src/policy.rs` | Parses minimal YAML via `AgentManifest::from_yaml_str`, compiles policy, asserts tools and `max_tool_calls` propagate. |
| `log_watcher_denies_outside_allowlist` | `tests/integration.rs` | Temp dir with an allowed subtree; `read_log` succeeds inside, fails outside; uses `ToolGateway` only. |
| `runner_writes_audit_and_summary` | `tests/integration.rs` | Full pipeline: manifest file + task file on disk, `runner::run`, asserts `audit.jsonl` and `summary.json` exist and the step succeeds. |

**Not** part of `cargo test` for the main crate: `testdata/sample-crate` is a tiny Rust fixture; its own `#[test]` runs only if you `cargo test` inside that crate.

## Quick command reference

```bash
cd agent-runtime

# Tests
cargo test

# CLI example (paths relative to agent-runtime/)
cargo run -- launch --manifest agents/log-watcher.yaml --task tasks/log-watcher.json

# GUI
cargo run --bin agent-runtime-gui
```
