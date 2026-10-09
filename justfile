set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# once per clone: local agent config (.claude/, .codex/) that is not tracked
setup:
    mkdir -p .claude .codex
    ln -sfn ../.agents/skills .claude/skills
    printf '{\n  "enabledMcpjsonServers": ["cyrus"]\n}\n' > .claude/settings.json
    printf '# Loaded only for a trusted project.\n[mcp_servers.cyrus]\ncommand = "cargo"\nargs = ["run", "-q", "-p", "cyrus-mcp"]\nenv = { CYRUS_CONTROL_ADDR = "127.0.0.1:7117" }\n' > .codex/config.toml

# fmt, clippy, tests
gates:
    dev/scripts/gates.sh

build:
    cargo build --workspace

run *ARGS:
    cargo run -p cyrus -- {{ARGS}}

run-headless *ARGS:
    cargo run -p cyrus -- --headless {{ARGS}}

# cyrus in dev/docker/dev (virtual display + software Vulkan), VibePod preview "cyrus"
run-novnc *ARGS:
    dev/scripts/run-game.sh {{ARGS}}

# the original game in dev/docker/retro, VibePod preview "cyrus-retro"
retro:
    dev/scripts/run-retro.sh

stop:
    -vibepod-preview stop cyrus
    -vibepod-preview stop cyrus-retro
    -vibepod-preview stop cyrus-web

mcp:
    cargo run -q -p cyrus-mcp

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
