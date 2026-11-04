# CPAL browser scheduling

Vendored from the published CPAL 0.17.3 crate, under Apache-2.0. Library sources,
build script and license are retained. Example manifest entries are omitted.

The WebAudio backend rebases expired buffer start times to the current audio
clock plus its existing 25 ms preparation margin. Otherwise buffers submitted
after main-thread delays start together and overlap. Future queued timestamps
remain unchanged. This prevents catch-up overlap; it cannot eliminate gaps while
the main thread is unable to produce audio. Native backends are unchanged.

The audio package's `browser_scheduling` integration test runs the clock contract
on native CI. Browser verification must also exercise actual expired
AudioBufferSourceNode starts and the production effects path. Remove this copy
when an upstream release provides equivalent scheduling recovery.
