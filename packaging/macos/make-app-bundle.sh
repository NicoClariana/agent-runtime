#!/usr/bin/env bash
# Build release GUI and wrap it in a minimal .app for macOS.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found. Install Rust: https://rustup.rs"
  exit 1
fi

cargo build --release --bin agent-runtime-gui

APP_NAME="Agent Runtime.app"
APP_DIR="$ROOT/$APP_NAME"
BIN="$ROOT/target/release/agent-runtime-gui"

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

cp "$BIN" "$APP_DIR/Contents/MacOS/agent-runtime-gui"
chmod +x "$APP_DIR/Contents/MacOS/agent-runtime-gui"
cp "$ROOT/packaging/macos/Info.plist" "$APP_DIR/Contents/Info.plist"

# Ship sample manifests and tasks inside the app so double‑click works without the repo.
for d in agents tasks testdata; do
  if [[ -d "$ROOT/$d" ]]; then
    cp -R "$ROOT/$d" "$APP_DIR/Contents/Resources/"
  fi
done

echo "Built: $APP_DIR"
echo "Open with: open \"$APP_DIR\""
echo "Sample agents/tasks are in Contents/Resources (bundled for Finder launches)."
