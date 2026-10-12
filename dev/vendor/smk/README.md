# smk: sequential browser input

Vendored from the published `smk` 0.1.0 package by Alessandro Rosetti,
[upstream repository](https://github.com/roarc0/smk). The existing libsmacker
1.2.0 Rust port and its file/memory APIs are retained. No media is included.

Cyrus adds `header_size`, `open_stream`, `packet_size`, and `decode_packet`
so the browser can supply complete compressed packets as they arrive, preserving
palette and delta-frame state without loading the whole movie. File navigation
is unavailable on a packet decoder; callers decode sequentially and discard
intermediate frames for forward skips. Header dimensions and allocation sizes
are checked before creating streaming buffers.

Run `cargo test --manifest-path dev/vendor/smk/Cargo.toml` from the workspace.
The installed frame/palette contract lives in `cyrus-assets::movie::stream`.

The source remains LGPL-2.1-or-later. See [LGPL-2.1.txt](LGPL-2.1.txt) and
[GPL-2.0.txt](GPL-2.0.txt).
