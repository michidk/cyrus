# Vendored CPAL

Vendored from the published `cpal` 0.17.3 crate, under Apache-2.0
([LICENSE](LICENSE)). Library sources, build script and license are retained;
example manifest entries are omitted. The workspace `[patch.crates-io]` points
`rodio` and the `cyrus` binary at this copy.

## Changes

- **Browser scheduling** (WebAudio backend): buffers whose start time has
  already passed are rebased to the current audio clock plus the existing
  25 ms preparation margin, so buffers submitted after a main-thread stall do
  not start together and overlap. Future timestamps are unchanged, and native
  backends follow upstream. This cannot fill gaps while the main thread
  produces no audio.
- **Mixer capture** (`src/diagnostic_output.rs`, Linux ALSA): when
  `CYRUS_MIXER_CAPTURE_DIR` names an empty directory, completed output blocks
  are copied before device submission into a bounded asynchronous queue, with
  generation time, sequence and sample format. Overflow, unsupported formats,
  several streams or a write failure invalidate the capture instead of
  retrying. Without the variable nothing is allocated or written. The control
  port's `capture_gameplay_frame` reports this as `pcm_generation`.

Replace the browser patch when an upstream release provides equivalent
recovery; port the diagnostic adapter before removing this copy.

## Testing

`cargo test -p cyrus-audio --test browser_scheduling` runs the clock contract
natively. Browser verification must also exercise real expired
`AudioBufferSourceNode` starts through the production effects path.
