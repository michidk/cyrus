#!/usr/bin/env bash
# Installs cargo-hauler (one shared cargo queue for all agents) and enables its
# hooks: Claude Code for this project only, Codex globally (Codex has no
# project scope). Re-run after a VibePod restart wipes ~/.local.
set -euo pipefail
cd "$(dirname "$0")/../.."
version=0.14.1
command -v npm >/dev/null || { echo "setup-hauler: no npm, skipping (builds use plain cargo)"; exit 0; }
node -e 'const [a,b]=process.versions.node.split(".").map(Number); process.exit(a>22||(a==22&&b>=19)?0:1)' \
    || { echo "setup-hauler: Node >= 22.19 needed, skipping"; exit 0; }
npm install -g --silent "cargo-hauler@$version"
root="$(npm root -g)/cargo-hauler/dist"
if command -v claude >/dev/null; then
    claude plugin marketplace list 2>/dev/null | grep -q cargo-hauler-marketplace \
        || claude plugin marketplace add "$root"
    # The installer's --scope project would bind to its own directory; run Claude's command here instead.
    claude plugin install cargo-hauler@cargo-hauler-marketplace --scope project
fi
command -v codex >/dev/null && cargo-hauler-install install codex
echo "setup-hauler: cargo-hauler $version ready; in Codex, trust the cargo-hauler hooks once when asked"
