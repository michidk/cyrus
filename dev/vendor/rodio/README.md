# Rodio queue startup

Vendored from the published `rodio` 0.22.2 crate from RustAudio, under MIT OR
Apache-2.0. Library source and licenses are retained; example and benchmark
manifest entries are omitted because their sources are not shipped here.

Conversion starts when the mixer first reads a source, so an empty player can
receive its first cue before its channel count and sample rate are fixed.
The first sample is retained before reading metadata so queued source and speed
changes are visible. Exact remaining size hints preserve finite source boundaries.
These changes prevent incorrect startup conversion and missing interpolated frames.
Cyrus retains decoded PCM size hints for the same reason. Queues use exact
remaining lengths to expose source transitions after playback and idle silence.
Finite silence reports its remaining span rather than its initial length.
Two upstream doc-comment lines also have trailing whitespace removed.
Feature-specific imports and loop settings are conditional, and rejected decoder
input is explicitly dropped, so builds without dither or Symphonia pass strict
warning checks.

The public backend contracts are tested in
`crates/cyrus-audio/tests/backend_queue.rs`. Remove this copy when a compatible
upstream release provides both fixes. See [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
