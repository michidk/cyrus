set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# fmt, clippy, tests, publish check
gates:
    scripts/gates.sh

build:
    cargo build --workspace

run *ARGS:
    cargo run -p cyrus -- {{ARGS}}

run-headless *ARGS:
    cargo run -p cyrus -- --headless {{ARGS}}

# cyrus in docker/dev (virtual display + software Vulkan), VibePod preview "cyrus"
run-novnc *ARGS:
    scripts/run-game.sh {{ARGS}}

# the original game in docker/retro, VibePod preview "cyrus-retro"
retro:
    scripts/run-retro.sh

stop:
    -vibepod-preview stop cyrus
    -vibepod-preview stop cyrus-retro
    -vibepod-preview stop cyrus-web

mcp:
    cargo run -q -p cyrus-mcp

docker-build:
    docker build -t cyrus-retro:dev docker/retro
    docker build -t cyrus-dev:dev docker/dev

# wasm build of crates/cyrus-web into target/web
web-build:
    scripts/build-web.sh

# serve the wasm build, VibePod preview "cyrus-web" (needs exported scenes in cyrus-config/cache)
run-web:
    scripts/run-web.sh
