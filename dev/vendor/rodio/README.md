# Vendored rodio

Vendored from the published `rodio` 0.22.2 crate from RustAudio, under MIT OR
Apache-2.0 ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
Library sources and licenses are retained; example and benchmark manifest
entries are omitted because their sources are not shipped here. The workspace
`[patch.crates-io]` points `cyrus-audio` and Bevy's audio at this copy.

## Changes

- **Queue startup**: conversion starts when the mixer first reads a source, so
  an empty player can receive its first cue before its channel count and sample
  rate are fixed. This copy reads the first sample before the metadata, and
  queues, finite silence and `SamplesBuffer` report exact remaining span
  lengths, so a converter never prefetches the next cue at the previous cue's
  rate and no interpolated frames are lost at source boundaries.
- **Optional sinc upsampling**: `ResamplingMethod::Sinc24` uses Rubato's
  fractional-reader API (24-tap Hann-windowed sinc, FFT features disabled)
  without per-frame allocation. `SampleRateConverter::new` stays linear;
  equal-rate and downsampled sources keep their original path. The game selects
  the method through its audio fidelity setting.
- **Build hygiene**: feature-specific imports are conditional so builds
  without dither or Symphonia pass strict warning checks.

Remove this copy when a compatible upstream release provides equivalent queue
startup and a selectable streaming converter.

## Testing

```sh
cargo test -p cyrus-audio --test backend_queue --test backend_resampling
```
