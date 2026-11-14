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

## Optional sinc upsampling

Cyrus adds an explicit `ResamplingMethod::Sinc24` path using Rubato 5.1's public
fractional-reader API. Rubato supplies the 24-tap Hann-windowed sinc, automatic
cutoff, convolution and fractional interpolation. FFT features are disabled.
The bounded source adapter preserves channel ordering, queue boundaries, cloned
continuations and the full source duration without per-frame allocations.
`SampleRateConverter::new` remains linear. The game selects the backend default
for subsequent source spans through its independent audio fidelity setting.
Equal-rate and downsampled sources retain their original path.

`SamplesBuffer::current_span_len` now reports its remaining samples, so a converter
cannot prefetch the next cue under the previous cue's rate. Public mixer and
filter contracts are tested in `crates/cyrus-audio/tests/backend_resampling.rs`.
The optional filter integration remains a local extension until upstream provides
an equivalent selectable streaming converter.
