set shell := ["bash", "-euo", "pipefail", "-c"]

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

# publish-check, fmt, clippy, tests
gates:
    dev/scripts/gates.sh

build:
    cargo build --workspace

run *ARGS:
    cargo run -p cyrus -- {{ARGS}}

run-headless *ARGS:
    cargo run -p cyrus -- --headless {{ARGS}}

# cyrus in dev/docker/dev (virtual display + software Vulkan), background container with the control port
run-native *ARGS:
    dev/scripts/run-game.sh {{ARGS}}

# the original game in dev/docker/retro, VibePod preview "cyrus-retro"
retro:
    dev/scripts/run-retro.sh

# stop the previews and the native container
stop:
    -vibepod-preview stop cyrus-retro
    -vibepod-preview stop cyrus-web
    -docker rm -f cyrus-dev

mcp:
    dev/scripts/mcp.sh

docker-build:
    docker build -t cyrus-retro:dev dev/docker/retro
    docker build -t cyrus-dev:dev dev/docker/dev

# wasm build of crates/cyrus-web into target/web
web-build:
    dev/scripts/build-web.sh

# serve the wasm build, VibePod preview "cyrus-web" (needs exported scenes in cyrus-config/cache)
run-web:
    dev/scripts/run-web.sh

# headless game + MCP server round-trip (needs an install and built binaries)
smoke-mcp:
    dev/scripts/smoke-mcp.sh


# same scene from cyrus and the original into cyrus-config/compare/<label>-{cyrus,original}.png
compare LABEL:
    dev/scripts/compare.sh {{LABEL}}

# reference clones under magic/reference (untracked): format docs and the Unity reimplementation
reference:
    mkdir -p magic/reference
    [ -d magic/reference/redguard-preservation ] || git clone -q https://github.com/michidk/redguard-preservation magic/reference/redguard-preservation
    [ -d magic/reference/redguard-unity ] || git clone -q https://github.com/RGUnity/redguard-unity magic/reference/redguard-unity
    git -C magic/reference/redguard-preservation pull -q --ff-only
    git -C magic/reference/redguard-unity pull -q --ff-only
    @echo "docs: magic/reference/redguard-preservation/docs  code: magic/reference/redguard-unity/Assets/Scripts"
