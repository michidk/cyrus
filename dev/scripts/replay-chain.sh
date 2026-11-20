#!/usr/bin/env bash
# Replay chapter inputs in a fresh session, using the player's read-only install.
set -euo pipefail
cd "$(dirname "$0")/../.."
source dev/scripts/toolchain.sh
install=$(realpath "${REDGUARD_DIR:-redguard}")
[[ -d "$install" ]] || { echo "replay-chain: install directory unavailable: $install" >&2; exit 1; }
config=${CYRUS_CONFIG_DIR:-magic/replay-chain}
binary=${CYRUS_REPLAY_BINARY:-$config/cyrus}
if [[ -z "${CYRUS_REPLAY_BINARY:-}" ]]; then
    CYRUS_NATIVE_EXECUTABLE="$binary" bash dev/scripts/build-native.sh
fi
runner_options=()
if [[ "${1:-}" == --keep-running ]]; then
    runner_options+=(--keep-running)
    shift
fi
if [[ $# -eq 0 ]]; then
    shopt -s nullglob
    chapters=(dev/fixtures/chapters/*.replay.json)
    [[ ${#chapters[@]} -gt 0 ]] || { echo 'replay-chain: no chapter fixtures' >&2; exit 1; }
    python3 -B dev/scripts/replay-chain.py --binary "$binary" --install "$install" --config "$config/chapters" "${runner_options[@]}" "${chapters[@]}"
    for segment in dev/fixtures/segments/*.replay.json; do
        python3 -B dev/scripts/replay-chain.py --binary "$binary" --install "$install" --config "$config/segments/$(basename "$segment" .replay.json)" "$segment"
    done
    exit 0
fi
[[ $# -gt 0 ]] || { echo 'replay-chain: no replay fixtures' >&2; exit 1; }
exec python3 -B dev/scripts/replay-chain.py --binary "$binary" --install "$install" --config "$config" "${runner_options[@]}" "$@"
