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
| **agent-runtime-gui** | Desktop UI (egui): orchestrator-style view, mode switch (scripted or mock-agent loop), policy snapshot, run artifacts. |

## Built-in tools

Tools must appear in `capabilities.tools` to be invocable.

| Path | Tools |
|------|--------|
| **Log** (no shell) | `list_logs`, `read_log`, `tail_log`, `grep_log` |
| **Tester** | `run_tests` (only allowlisted absolute command, `working_directory` under read policy) |
| **Optional git** | `git_status` (only if `capabilities.git: true` and git binary allowlisted) |

## Sandbox stance (MVP)

Treat this as a **policy-enforced runtime**, not a **hardened sandbox**.

**What you have today**

- One process, structured tools, allowlisted subprocesses, checks in Rust before I/O or spawn.
- Path checks use `paths::resolve_user_path` (cwd + lexical `..` normalization) plus `std::fs::canonicalize` so symlink targets outside an allowlisted directory are rejected (see `tests/path_policy.rs`).

**What is still weak (do not oversell)**

1. **No OS-level isolation** — bugs in path handling, subprocess wiring, or argument validation could still break policy. Containers/seccomp/cgroups would be a separate layer.
2. **Subprocess risk** — Even allowlisted binaries (e.g. `git`) bundle many behaviors; narrow further with fixed argument profiles, not free-form args.
3. **`network`** — Mostly metadata; not enforced against real network use.
4. **`max_memory_mb`** — Recorded in policy but not enforced as an OS limit.
5. **TOCTOU** — A path can change between policy check and read; stronger designs re-check or open via verified file descriptors.

The manifest model is intended to stay valid when you add real sandboxing later.

## Honest limitations & roadmap (suggested order)

1. **File access** — Keep hardening path logic; add more adversarial tests (long symlink chains, racey renames, etc.).
2. **Subprocess semantics** — Move from “allowlisted absolute command + free args” toward **named profiles** (fixed executable + arg prefix + allowed workdirs).
3. **Runtime limits** — Real subprocess timeout + kill + cleanup; then memory/CPU/open-file limits where the OS allows.
4. **Typed tool args/results** — Replace untyped `serde_json::Value` per tool with structs for validation and audits.
5. **Layering** — Keep a strict split: policy evaluation vs tool execution vs runner lifecycle vs append-only audit.

**Defer:** LLM integration, multi-agent choreography, many tools/plugins, “smart” permission inference from role text.

**Next milestone worth targeting:** hostile task inputs + hostile filesystem layout without policy escape (then golden audit tests, fuzzing, manual red-team checklist).

## Testing strategy (layered)

| Layer | Purpose |
|-------|---------|
| Policy unit tests | Compiled policy: tools, paths, commands, limits, invalid manifests. |
| Gateway tests | Per tool: allowed + denied; path traversal, symlinks, bad args. |
| Runner integration | Lifecycle, audit on failure, timeouts (when implemented). |
| Adversarial / negative | Deliberately break the runtime; add a regression test for each finding. |
| Golden audit | Fixed manifest + task → stable event order and denial shapes. |
| Fuzzing / property tests | Paths, manifest/task parsing, regex-like inputs (when added). |
| Manual red-team | Checklist before claiming “sandbox”. |

## Path resolution (for policy checks)

`src/paths.rs` joins relative paths with the process current directory and lexically removes `.` / `..`. `CompiledPolicy` then compares using `canonicalize` when possible so a symlink inside an allowed directory cannot point at data outside that directory’s resolved tree. Non-existent file paths climb to an existing ancestor, verify that ancestor is under a policy root, and require remaining path components to be normal names (no `..`).

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

## Agent loop (new)

The runtime now includes a simple agent-session bridge:

- `model::ModelClient` trait for model backends.
- `model::types::AgentDecision` (`tool_call` or `final`).
- `model::mock::MockModelClient` for deterministic end-to-end testing.
- `agent::run_agent()` loop:
  1. load manifest + prompt,
  2. compile policy,
  3. expose only allowed tool schemas,
  4. ask model for next decision,
  5. route tool calls through `ToolGateway`,
  6. append results back into conversation,
  7. stop on `Final` or limits.

Artifacts for agent-loop runs include `conversation.json` (decision trace) in addition to `audit.jsonl`.

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
| `paths::tests::*` | `src/paths.rs` | Empty path rejected; `..` normalized relative to cwd. |
| `policy::tests::compile_collects_tools` | `src/policy.rs` | Parses minimal YAML via `AgentManifest::from_yaml_str`, compiles policy, asserts tools and `max_tool_calls` propagate. |
| `log_watcher_denies_outside_allowlist` | `tests/integration.rs` | Temp dir with an allowed subtree; `read_log` succeeds inside, fails outside; uses `ToolGateway` only. |
| `runner_writes_audit_and_summary` | `tests/integration.rs` | Full pipeline: manifest file + task file on disk, `runner::run`, asserts `audit.jsonl` and `summary.json` exist and the step succeeds. |
| `dotdot_escape_sibling_directory_denied` | `tests/path_policy.rs` | `../` traversal from an allowed subtree to a sibling path is denied. |
| `symlink_inside_allowlist_to_outside_target_denied` | `tests/path_policy.rs` | Unix: symlink under allowlist pointing outside does not allow `read_log`. |
| `allowed_file_via_normalized_path_ok` | `tests/path_policy.rs` | Redundant `..` segments still reach an allowed file. |
| `empty_path_string_denied` | `tests/path_policy.rs` | Empty path rejected. |

**Not** part of `cargo test` for the main crate: `testdata/sample-crate` is a tiny Rust fixture; its own `#[test]` runs only if you `cargo test` inside that crate.

## Quick command reference

```bash
cd agent-runtime

# Tests
cargo test

# CLI example (paths relative to agent-runtime/)
cargo run --bin agent-runner -- launch --manifest agents/log-watcher/manifest.yaml --task tasks/log-watcher.json

# Agent loop with deterministic mock model
cargo run --bin agent-runner -- run-agent-mock \
  --manifest agents/log-watcher/manifest.yaml \
  --prompt agents/log-watcher/prompt.md \
  --message "Analyze logs for failures and summarize incidents."

# GUI
cargo run --bin agent-runtime-gui
```
