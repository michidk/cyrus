#!/usr/bin/env bash
# Install-backed scheduled verification; the scheduler supplies persistent paths.
set -euo pipefail
if [[ "${1:-}" != --snapshot ]]; then
    exec bash -c "$(cat "$0")" "$0" --snapshot "$@"
fi
shift
cd "$(dirname "$0")/../.."
: "${CYRUS_CONFIG_DIR:?set a persistent nightly state directory}"
: "${REDGUARD_DIR:?set the read-only install directory}"
mkdir -p "$CYRUS_CONFIG_DIR/nightly"
exec 9>"$CYRUS_CONFIG_DIR/nightly.lock"
flock -n 9 || exit 0
run="$CYRUS_CONFIG_DIR/nightly/run-$(date -u +%Y%m%dT%H%M%S%NZ)"
mkdir -p "$run"
python3 - "$CYRUS_CONFIG_DIR/nightly" <<'PY'
from pathlib import Path
import shutil
import sys
root = Path(sys.argv[1])
for log in root.glob("*.log"):
    destination = root / f"run-{log.stem}"
    destination.mkdir(exist_ok=True)
    log.rename(destination / "replay.log")
    marker = root / "unfinished.issue.md"
    if marker.exists():
        marker.write_text(marker.read_text().replace(str(log), str(destination / "replay.log")))
for old in sorted(root.glob("run-*"), reverse=True)[2:]:
    if old.is_dir():
        shutil.rmtree(old)
for body in root.glob("*.log.issue.md"):
    body.unlink()
PY
log="$run/replay.log"
runtime="$CYRUS_CONFIG_DIR/runtime"
rm -rf -- "$runtime"
unfinished="$CYRUS_CONFIG_DIR/nightly/unfinished.issue.md"
if [[ -f "$unfinished" ]]; then
    gh issue comment 322 --repo michidk/cyrus --body-file "$unfinished"
    rm "$unfinished"
fi
report_failure() {
    status=$?
    if [[ -f "$runtime/process.log" ]]; then
        cp "$runtime/process.log" "$run/process.log"
    fi
    rm -rf -- "$runtime"
    if [[ $status -ne 0 ]]; then
        body="$log.issue.md"
        {
            echo 'Install-backed nightly replay failed.'
            echo "Commit: $(git rev-parse HEAD). Exit status: $status."
            echo "Host evidence: $log"
            echo 'Replay and process logs are retained for the last two runs.'
            echo 'No fixture expectations or game behavior were changed by this run.'
        } > "$body"
        if gh issue comment 322 --repo michidk/cyrus --body-file "$body"; then
            rm -f "$unfinished"
        fi
        rm -f "$body"
    else
        body="$log.issue.md"
        {
            echo 'Install-backed nightly replay passed end to end.'
            echo "Commit: $(git rev-parse HEAD). Exit status: 0."
            echo "Host evidence: $log"
            echo 'All configured chapter fixtures and checkpoints passed.'
        } > "$body"
        gh issue comment 322 --repo michidk/cyrus --body-file "$body"
        rm -f "$body" "$unfinished"
    fi
}
trap report_failure EXIT
{
    echo 'Previous install-backed nightly replay was interrupted before completion.'
    echo "Started from commit: $(git rev-parse HEAD)."
    echo "Host evidence: $log"
    echo 'The previous run is not counted as passing; retrying from New Game.'
} > "$unfinished"
exec > >(tee "$log") 2>&1
git pull --rebase origin main
if ! command -v cargo-hauler >/dev/null 2>&1; then
    just setup
fi
export CARGO_HAULER_ALLOW_SHARED_TARGET=1
source dev/scripts/toolchain.sh
source dev/scripts/cargo.sh
cargo build -p cyrus --bin cyrus
CYRUS_REPLAY_BINARY="${CARGO_TARGET_DIR:?shared Cargo target must be configured}/debug/cyrus" \
    CYRUS_CONFIG_DIR="$runtime" just replay-chain
