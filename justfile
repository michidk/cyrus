# bash -euo pipefail, with the rootless toolchain from `just toolchain` when cargo is not on PATH
set shell := ["dev/scripts/toolchain.sh", "-c"]

default:
    @just --list

# once per clone: the publish-check pre-commit hook, local agent config (.claude/, .codex/) and cargo-hauler
setup:
    git config core.hooksPath dev/hooks
    mkdir -p .claude .codex
    ln -sfn ../.agents/skills .claude/skills
    printf '{\n  "enabledMcpjsonServers": ["cyrus"]\n}\n' > .claude/settings.json
    printf '# Loaded only for a trusted project.\n[mcp_servers.cyrus]\ncommand = "dev/scripts/mcp.sh"\nenv = { CYRUS_CONTROL_ADDR = "127.0.0.1:7117" }\n' > .codex/config.toml
    dev/scripts/setup-hauler.sh

# rootless Rust toolchain, wasm target, wasm-bindgen and Bevy's Linux libraries under magic/build-tools (no sudo)
toolchain:
    dev/scripts/toolchain.sh install

# publish-check, generated map, fmt, clippy, tests
gates:
    dev/scripts/gates.sh

# regenerate docs/MAP.md from the sources
genmap:
    dev/scripts/genmap.sh

build:
    cargo build --workspace

# build the bundled native mods (mods/*) for wasm32-wasip2 and install them into cyrus-config/mods
mods:
    for dir in mods/*/; do \
        id="$(basename "$dir")"; \
        (cd "$dir" && CARGO_TARGET_DIR="$PWD/../../target/mods" cargo build --release --target wasm32-wasip2); \
        mkdir -p "cyrus-config/mods/$id"; \
        cp "$dir/mod.toml" "cyrus-config/mods/$id/"; \
        cp "target/mods/wasm32-wasip2/release/${id//-/_}.wasm" "cyrus-config/mods/$id/mod.wasm"; \
        if [ -f "$dir/water_surface.wesl" ]; then cp "$dir/water_surface.wesl" "cyrus-config/mods/$id/"; fi; \
    done

run *ARGS:
    cargo run -p cyrus -- {{ARGS}}

run-headless *ARGS:
    cargo run -p cyrus -- --headless {{ARGS}}

# cyrus in dev/docker/dev (virtual display + software Vulkan), background container with the control port
run-native *ARGS:
    dev/scripts/run-game.sh {{ARGS}}

# the original game in dev/docker/retro, VibePod preview "cyrus-retro"
retro *ARGS:
    dev/scripts/run-retro.sh {{ARGS}}

# stop the previews and the native container
stop:
    -vibepod-preview stop cyrus-retro
    -vibepod-preview stop cyrus-web
    -docker rm -f cyrus-dev

# the MCP server for agents; its own target dir so it never waits for the game's build lock
mcp:
    dev/scripts/mcp.sh

docker-build:
    docker build -t cyrus-retro:dev dev/docker/retro
    docker build -t cyrus-dev:dev dev/docker/dev

# wasm build of crates/cyrus-web into target/web
web-build:
    dev/scripts/build-web.sh

# serve the wasm build, VibePod preview "cyrus-web" (discovers an install or accepts --install PATH)
run-web *ARGS:
    dev/scripts/run-web.sh cyrus-web {{ARGS}}

# headless game + MCP server round-trip (needs an install and built binaries)
smoke-mcp:
    dev/scripts/smoke-mcp.sh


# current views from cyrus and the original into magic/captures/legacy/
compare LABEL:
    dev/scripts/compare.sh {{LABEL}}

# matched JSON triplets, or legacy JSONL control replay and comparison
replay FIXTURE LABEL:
    dev/scripts/replay.sh {{FIXTURE}} {{LABEL}}

# reference clones under magic/reference (untracked): format docs and the Unity reimplementation
reference:
    mkdir -p magic/reference
    [ -d magic/reference/redguard-preservation ] || git clone -q https://github.com/michidk/redguard-preservation magic/reference/redguard-preservation
    [ -d magic/reference/redguard-unity ] || git clone -q https://github.com/RGUnity/redguard-unity magic/reference/redguard-unity
    git -C magic/reference/redguard-preservation pull -q --ff-only
    git -C magic/reference/redguard-unity pull -q --ff-only
    @echo "docs: magic/reference/redguard-preservation/docs  code: magic/reference/redguard-unity/Assets/Scripts"

# Independent asset viewer; native launches accept --install or discover an install.
viewer *ARGS:
    cargo run -p cyrus-viewer -- {{ARGS}}

viewer-web-build:
    dev/scripts/build-web.sh cyrus-viewer

# Serve the viewer and an explicit read-only original install.
run-viewer-web *args:
    dev/scripts/run-web.sh cyrus-viewer {{args}}
