#!/usr/bin/env bash
# Starts the cyrus MCP server for Claude Code and Codex. Runs the built binary,
# so a busy build queue or a tree that does not compile cannot keep the agent
# tooling down; builds it once when missing. stdout is the MCP stream, so build
# output goes to stderr.
set -euo pipefail
cd "$(dirname "$0")/../.."
bin=target/debug/cyrus-mcp
if [ ! -x "$bin" ]; then
    . dev/scripts/cargo.sh
    cargo build -q -p cyrus-mcp >&2
fi
exec "$bin" "$@"
