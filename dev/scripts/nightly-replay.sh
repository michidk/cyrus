#!/usr/bin/env bash
# Install-backed scheduled verification; the scheduler supplies persistent paths.
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${CYRUS_CONFIG_DIR:?set a persistent nightly state directory}"
: "${REDGUARD_DIR:?set the read-only install directory}"
mkdir -p "$CYRUS_CONFIG_DIR/nightly"
exec 9>"$CYRUS_CONFIG_DIR/nightly.lock"
flock -n 9 || exit 0
log="$CYRUS_CONFIG_DIR/nightly/$(date -u +%Y%m%dT%H%M%SZ).log"
unfinished="$CYRUS_CONFIG_DIR/nightly/unfinished.issue.md"
if [[ -f "$unfinished" ]]; then
    gh issue comment 322 --repo michidk/cyrus --body-file "$unfinished"
    rm "$unfinished"
fi
report_failure() {
    status=$?
    if [[ $status -ne 0 ]]; then
        body="$log.issue.md"
        {
            echo 'Install-backed nightly replay failed.'
            echo "Commit: $(git rev-parse HEAD). Exit status: $status."
            echo "Host evidence: $log"
            echo 'Checkpoint observations and process.log are retained in the nightly config directory.'
            echo 'No fixture expectations or game behavior were changed by this run.'
        } > "$body"
        if gh issue comment 322 --repo michidk/cyrus --body-file "$body"; then
            rm -f "$unfinished"
        fi
    else
        rm -f "$unfinished"
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
just replay-chain
