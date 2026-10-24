#!/usr/bin/env bash
# The tracked tree is the product only. This checks the git index (what the
# next commit or push contains) for a fixed set of shapes: local-only paths,
# game data file types, decompiler placeholder names and address-shaped hex.
# It is a grep, not a proof: passing says those shapes are absent, nothing
# about where code came from. A line that legitimately needs a long hex
# constant carries the marker `publish-check: allow`.
set -euo pipefail
cd "$(dirname "$0")/../.."

fail=0

paths=$(git ls-files | grep -E '^(magic|game-config|redguard)(/|$)' || true)
game_files=$(git ls-files | grep -iE '\.(rgm|rob|wld|pvo|col|gxa|rtx|sfx|fnt|bsi|3dc|cht|sav|gog|ins)$' || true)
traces=$(git grep --cached -nIE '\b(FUN|DAT|LAB|PTR|SUB)_[0-9a-fA-F]{4,}\b|\b0x[0-9a-fA-F]{6,}\b' \
    -- ':!dev/scripts/publish-check.sh' | grep -v 'publish-check: allow' || true)

if [ -n "$paths" ]; then
    echo "publish-check: local-only paths are tracked (magic/, game-config/, redguard):"
    echo "$paths"
    fail=1
fi
if [ -n "$game_files" ]; then
    echo "publish-check: game data files are tracked:"
    echo "$game_files"
    fail=1
fi
if [ -n "$traces" ]; then
    echo "publish-check: decompiler names or address-shaped hex in tracked files:"
    echo "$traces"
    fail=1
fi

[ "$fail" = 0 ] && echo "publish-check: clean"
exit "$fail"
