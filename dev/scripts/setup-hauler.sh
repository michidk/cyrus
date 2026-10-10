#!/usr/bin/env bash
# Installs cargo-hauler (one shared cargo queue for all agents) and enables its
# hooks: Claude Code for this project only, Codex globally (Codex has no
# project scope). Re-run after a VibePod restart wipes ~/.local, for example
# from a VibePod @startup schedule.
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
if command -v codex >/dev/null; then
    # Codex dropped plugin hooks, so register them as user hooks from a copy in
    # ~/.codex, which survives restarts; the path is stable per version, so the
    # trust Codex asks for once stays valid.
    copy="$HOME/.codex/hooks/cargo-hauler-$version"
    [ -d "$copy" ] || cp -a "$root" "$copy"
    python3 -I - "$copy" <<'PY'
import json, os, sys
root = sys.argv[1]
path = os.path.expanduser("~/.codex/hooks.json")
config = json.load(open(path)) if os.path.exists(path) else {}
hooks = config.setdefault("hooks", {})
for event in hooks:
    hooks[event] = [group for group in hooks[event] if "cargo-hauler" not in json.dumps(group)]
for event, route, timeout, matcher in [("SessionStart", "session-start", 5, None),
                                       ("PreToolUse", "tool-before", 10, "Bash"),
                                       ("PostToolUse", "tool-after", 10, "Bash"),
                                       ("Stop", "stop", 900, None)]:
    group = {"hooks": [{"type": "command",
                        "command": f"node '{root}/hooks/event-route-{route}.codex.mjs'",
                        "timeout": timeout}]}
    if matcher:
        group["matcher"] = matcher
    hooks.setdefault(event, []).append(group)
json.dump(config, open(path, "w"), indent=2)
PY
fi
echo "setup-hauler: cargo-hauler $version ready; Codex asks once to trust its hooks after a version change"
