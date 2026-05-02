#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${PORT:-8000}"
PROFILE="${PROFILE:-release}"

cd "$ROOT_DIR"

echo "Building editor_server (profile: $PROFILE)..."
if [[ "$PROFILE" == "release" ]]; then
  cargo build --release --bin editor_server
  BIN="$ROOT_DIR/target/release/editor_server"
else
  cargo build --bin editor_server
  BIN="$ROOT_DIR/target/debug/editor_server"
fi

echo "Serving Rust editor server at http://127.0.0.1:${PORT}/"
EDITOR_BIND="127.0.0.1:${PORT}" exec "$BIN"
