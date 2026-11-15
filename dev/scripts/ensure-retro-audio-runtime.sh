#!/usr/bin/env bash
# Retain reference compiler inputs and objects across ephemeral Docker resets.
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
revision=59915c19e52afc30f152d210bc5b2e06e7c320c6
checksum=8b23c1f7f46bf89f62635a75865846adbd75c3587d06b7b97c968d9fe2f34d3c
options=(--enable-sdl2 --disable-debug --disable-avcodec --disable-libfluidsynth --disable-libslirp)
fingerprint=$({ printf '%s\n' "$revision" "$checksum" "${options[@]}"; \
    cat "$repo/dev/docker/retro/Dockerfile.audio" "$repo/dev/docker/retro/Dockerfile.audio-build" \
    "$repo/dev/docker/retro/cd-volume.patch" "$repo/dev/docker/retro/entrypoint.sh" \
    "$repo/dev/docker/retro/dosbox-x.conf"; } | sha256sum | cut -d' ' -f1)
image="cyrus-retro-audio:$fingerprint"
cache=$(realpath -m "$repo/magic/build-tools")
archive="$cache/cyrus-retro-audio-$fingerprint.tar"
mkdir -p "$cache"
# Serialize preparation independently of the live-game lock.
exec 7>"$archive.build.lock"
flock 7
if ! docker image inspect "$image" >/dev/null 2>&1; then
    if [[ -s "$archive" ]]; then
        "$repo/dev/scripts/ensure-runtime-image.sh" "$image" "$archive" >&2
    else
        tools_key=$(sha256sum "$repo/dev/docker/retro/Dockerfile.audio-build" | cut -d' ' -f1)
        tools="cyrus-retro-audio-build:$tools_key"
        tools_archive="$cache/cyrus-retro-audio-build-$tools_key.tar"
        if ! docker image inspect "$tools" >/dev/null 2>&1; then
            if [[ -s "$tools_archive" ]]; then
                "$repo/dev/scripts/ensure-runtime-image.sh" "$tools" "$tools_archive" >&2
            else
                docker build -t "$tools" -f "$repo/dev/docker/retro/Dockerfile.audio-build" "$repo" >&2
            fi
        fi
        "$repo/dev/scripts/ensure-runtime-image.sh" --retain "$tools" "$tools_archive" >&2
        source_archive="$cache/dosbox-x-$revision.tar.gz"
        if [[ ! -s "$source_archive" ]]; then
            curl --fail --location --output "$source_archive.partial" --connect-timeout 20 --max-time 180 \
                "https://codeload.github.com/joncampbell123/dosbox-x/tar.gz/$revision" >&2
            printf '%s  %s\n' "$checksum" "$source_archive.partial" | sha256sum --check >&2
            mv "$source_archive.partial" "$source_archive"
        fi
        printf '%s  %s\n' "$checksum" "$source_archive" | sha256sum --check >&2
        source_key=$({ printf '%s\n' "$revision" "$checksum" "${options[@]}"; cat "$repo/dev/docker/retro/cd-volume.patch" \
            "$repo/dev/docker/retro/Dockerfile.audio-build"; } | sha256sum | cut -d' ' -f1)
        work="$cache/retro-audio-$source_key"
        exec 6>"$work.compile.lock"
        flock 6
        if [[ ! -d "$work" ]]; then
            mkdir -p "$work.partial"
            tar -xzf "$source_archive" -C "$work.partial" --strip-components=1
            patch --batch --fuzz=0 -d "$work.partial" -p1 < "$repo/dev/docker/retro/cd-volume.patch" >&2
            mv "$work.partial" "$work"
        fi
        docker run --rm -v "$work:/src" -w /src --entrypoint bash "$tools" -ec '
            test -x configure || ./autogen.sh
            if ! test -s config.status; then
                ./configure "$@"
            fi
            make -j2
            mkdir -p cyrus-output
            cp src/dosbox-x cyrus-output/dosbox-x.partial
            strip cyrus-output/dosbox-x.partial
            mv cyrus-output/dosbox-x.partial cyrus-output/dosbox-x
        ' -- "${options[@]}" >&2
        flock -u 6
        docker build --build-context "dosbox-build=$work/cyrus-output" \
            -t "$image" -f "$repo/dev/docker/retro/Dockerfile.audio" "$repo" >&2
    fi
fi
marker=$(docker run --rm --entrypoint cat "$image" /etc/cyrus/audio-reference)
if [[ "$marker" != single-cd-volume-v1 ]]; then
    echo "original audio runtime has no single-application CD volume contract: $image" >&2
    exit 1
fi
"$repo/dev/scripts/ensure-runtime-image.sh" --retain "$image" "$archive" >&2
printf '%s\n' "$image"
