#!/usr/bin/env bash
# Refuses tracked files that carry traces of binary research or game data.
set -euo pipefail
cd "$(dirname "$0")/.."
status=0
# Decompiler placeholder names and raw addresses in code or docs.
if git ls-files -z | xargs -0 grep -nE '\b(FUN|DAT|LAB|thunk_FUN)_[0-9a-fA-F]{6,}\b|\b0x[0-9a-fA-F]{6,8}\b' -- 2>/dev/null; then
    echo "publish-check: decompiler names or raw addresses found" >&2
    status=1
fi
# Game files by extension.
if git ls-files | grep -iE '\.(3d|3dc|rob|rgm|wld|col|pvo|gxa|bsi|sfx|rtx|fnt|cht|gog|ins|exe|swg|swx)$'; then
    echo "publish-check: game files tracked" >&2
    status=1
fi
# Anything under magic/ or cyrus-config/.
if git ls-files | grep -E '^(magic|cyrus-config)/'; then
    echo "publish-check: working memory tracked" >&2
    status=1
fi
[ $status -eq 0 ] && echo "publish-check: clean"
exit $status
