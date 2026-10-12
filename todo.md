# Asset viewer TODO

User-requested backlog from the viewer feedback and screenshots. The requested changes and investigations are complete. The findings below
record verification and remaining format or playback limitations.
Screenshot observations describe the viewer, not verified original-game behavior.

## Browsing and layout

- [x] **Default to the first available palette.** Put **Without palette** last
  in the dropdown. Apply this consistently to image and model previews; retain
  explicit palette selection and asset links. Images offer their embedded palette
  first when present, preserving authored colors. This supersedes the earlier default.
- [x] **Open single-member BSI content directly.** The user reports that BSI
  folders each lead to only one item. Check the indexed member count and avoid
  the extra navigation for single-member archives; keep member browsing for
  multi-member archives. Do not assume all BSI files have one member.
- [x] **Restore Other files.** List otherwise unclassified original files again,
  with file information and an explicit explanation when preview is unsupported.
  This supersedes the earlier request to hide them.
- [x] **Use the available content width.** In the `maps/BRENNANS.RGM`
  screenshot, the decoded-data tree occupies a narrow column while much of the
  center is unused. Let data, text, waveforms, and visual previews fill the space
  left by the navigation and details panels; preserve resizable panels.

## Images and animation

- [x] **Investigate BMP decoding failure.** Selecting `system/pointers.bmp`
  shows “Preview unavailable” and this error:
  `system/pointers.bmp: Format error decoding Bmp: Bitmap header too small (0 bytes): Bitmap header too small (0 bytes)`.
  Check the original bytes, browser reads, and decoder support before assigning
  the cause. A zero-byte header error alone does not establish an empty file.
- [x] **Add TEXBSI playback controls.** Provide Play/Pause, Loop, and an editable
  time between frames with explicit units. Keep manual frame inspection and
  show the current frame while playing.
- [x] **Support ROB playback where animation is available.** Provide the same
  playback, loop, and frame-timing controls for supported animated ROB content.
  Unsupported animation must be identified explicitly.
- [x] **Make animated models visibly animate.** The user reports no playback;
  `fxart/AVIKA001.3DC` is shown in a static arms-out pose with
  `fxart/ISLAND.COL`. The visible toolbar offers Edges, Frame object, and palette
  selection, but no animation controls. Check frame decoding and playback, and
  expose playback/loop/frame-timing controls. The still image alone cannot prove
  that playback is stalled.

## Archive contents and audio

- [x] **Investigate empty ROB member lists.** Opening `fxart/DRINT.ROB` shows
  `0 results` and blank content in the screenshot. Determine whether indexing,
  filtering, or parsing is responsible; distinguish loading, failure, and a
  genuinely empty archive in the UI.
- [x] **Investigate empty SFX member lists.** Opening `sound/MAIN.SFX` likewise
  shows `0 results` and blank content. Verify the archive index and list its
  supported sound entries, with errors reported instead of appearing empty.
- [x] **Investigate empty speech archive lists.** Opening `ENGLISH.RTX` shows
  `0 results` and blank content. List the speech index when the archive opens,
  then load only the selected speech resource. Distinguish indexing failure
  from an empty archive.
- [x] **Check WAV discovery completeness.** The WAV category shows one result,
  `sound/R212.WAV`, with waveform and playback controls. The user expects more.
  Compare the install inventory, manifest, and category filtering; distinguish
  standalone WAV files from sounds inside SFX/RTX archives and CD audio. The
  displayed count alone does not prove files are missing.
- [x] **Investigate movie loading.** The `disc/INTRO.SMK` screenshot shows
  “Loading selected asset…” with a spinner. Check time to first frame, playback,
  responsiveness, and progress/error reporting. No elapsed time is visible, so
  a permanent hang is not established by this screenshot.

## Presentation

- [x] **Show scripts and text as readable source.** Present SOUP scripts in DEF
  as scripts rather than only generic data trees. Give INI, LST, and TXT readable
  text views with appropriate formatting. Inspect scripts without executing them.
- [x] **Improve model inspection lighting.** `fxart/CRAK0009.3D` with
  `fxart/REDCAVE.COL` appears nearly black against the dark background, making
  geometry difficult to inspect. Check lighting, palette, materials, and normals;
  provide useful neutral viewer lighting without silently changing palette data.
  The screenshot does not isolate the cause.
- [x] **Visualize world geometry and PVO data.** Add useful spatial views based
  on supported preservation semantics. Let users switch between visualization
  and text/JSON views of the decoded data; label unsupported interpretations.

## Implementation constraints and upstream fixes

- [x] **Preserve runtime, browser-side parsing and lazy loading.** Builds must
  require no game files. The web viewer must work without a parsing server.
  List archives first, index each when opened, and load payloads on selection.
- [x] **Fix proven preservation defects upstream.** For each issue above,
  distinguish viewer presentation/loading bugs from `rgpre` format or parser
  defects. Where redguard-preservation is responsible, fix it there and create
  a pull request with reproducible evidence and focused tests; reference that
  PR from the viewer change. Do not open speculative fixes based only on these
  screenshots. Keep game files, screenshots, and raw research out of commits.

## Findings and verification limits

- **Verified, install inventory:** there is one standalone WAV, `sound/R212.WAV`.
  The viewer explains that SFX, RTX, and CD audio provide additional sounds.
- **Verified, source bytes:** `system/pointers.bmp` contains 6,880 bytes, a zero
  DIB-header length, and a header file size that disagrees with its actual size.
  It remains unsupported. The error now includes source size and context; the
  viewer does not modify or invent replacement artwork.
- **Verified, browser worker:** archive indexing returned 289 DRINT.ROB members,
  118 MAIN.SFX members, and 4,866 ENGLISH.RTX members. Selected SFX and speech
  resources decoded as audio. These transport checks do not prove UI playback.
- Model frame conversion is shared through preservation
  [PR #51](https://github.com/michidk/redguard-preservation/pull/51).
  Synthetic frame tests, the upstream test suite, formatting, and clippy passed.
- Movie preparation still reads the entire selected movie and prepares its
  first audio track before playback. Progress and errors are explicit; streaming
  playback is not implemented.
- The viewer uses generic inspection lighting and user-selected frame timing.
  Original-game lighting, animation timing, and visual fidelity are not verified.

## Completed behavior and checks

- Browsing retains Other files, separates canonical extensions, distinguishes
  archive indexing/errors from empty results, and opens singleton BSI images.
- Images and models provide Play/Pause, Loop, manual frame selection, and a frame
  interval in milliseconds. Compressed Avik geometry exposes all 124 frames.
- Model inspection has neutral lighting. Source and JSON views fill the content
  pane. Toolbars wrap, and model viewports stay inside the central panel.
- RGM and WLD offer geometry plus JSON. PVO offers a depth-controlled X/Z slice,
  opens on a populated layer, and provides JSON inspection.
- Selected-resource progress and a bounded transport retry accompany browser-side
  parsing. Service-worker cache writes no longer delay the requested response.
- `just gates` passed with the install available and unavailable. Browser-target
  clippy passed. The worker contract covers index-only startup, selected speech,
  chunking, retries, progress, and range validation. The service-worker contract
  checks that a delayed cache write does not hold the response.
- Shared-browser checks observed Avik frame changes, the first model palette,
  Without palette last, improved cave-model lighting, readable INI source,
  singleton BSI navigation, and movie frame advancement. Worker checks prepared
  RGM/WLD geometry with JSON and CATACOMB PVO cells entirely in the browser.
  Browser rendering used SwiftShader; normal-GPU performance and audible output
  were not assessed.
