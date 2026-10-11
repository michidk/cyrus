# Asset viewer TODO

User-requested backlog from the viewer feedback and screenshots. All items are
open; this document does not claim fixes or confirmed parser diagnoses.
Screenshot observations describe the viewer, not verified original-game behavior.

## Browsing and layout

- [ ] **Default to the first available palette.** Put **Without palette** last
  in the dropdown. Apply this consistently to image and model previews; retain
  explicit palette selection and asset links. This supersedes the earlier default.
- [ ] **Open single-member BSI content directly.** The user reports that BSI
  folders each lead to only one item. Check the indexed member count and avoid
  the extra navigation for single-member archives; keep member browsing for
  multi-member archives. Do not assume all BSI files have one member.
- [ ] **Restore Other files.** List otherwise unclassified original files again,
  with file information and an explicit explanation when preview is unsupported.
  This supersedes the earlier request to hide them.
- [ ] **Use the available content width.** In the `maps/BRENNANS.RGM`
  screenshot, the decoded-data tree occupies a narrow column while much of the
  center is unused. Let data, text, waveforms, and visual previews fill the space
  left by the navigation and details panels; preserve resizable panels.

## Images and animation

- [ ] **Investigate BMP decoding failure.** Selecting `system/pointers.bmp`
  shows “Preview unavailable” and this error:
  `system/pointers.bmp: Format error decoding Bmp: Bitmap header too small (0 bytes): Bitmap header too small (0 bytes)`.
  Check the original bytes, browser reads, and decoder support before assigning
  the cause. A zero-byte header error alone does not establish an empty file.
- [ ] **Add TEXBSI playback controls.** Provide Play/Pause, Loop, and an editable
  time between frames with explicit units. Keep manual frame inspection and
  show the current frame while playing.
- [ ] **Support ROB playback where animation is available.** Provide the same
  playback, loop, and frame-timing controls for supported animated ROB content.
  Unsupported animation must be identified explicitly.
- [ ] **Make animated models visibly animate.** The user reports no playback;
  `fxart/AVIKA001.3DC` is shown in a static arms-out pose with
  `fxart/ISLAND.COL`. The visible toolbar offers Edges, Frame object, and palette
  selection, but no animation controls. Check frame decoding and playback, and
  expose playback/loop/frame-timing controls. The still image alone cannot prove
  that playback is stalled.

## Archive contents and audio

- [ ] **Investigate empty ROB member lists.** Opening `fxart/DRINT.ROB` shows
  `0 results` and blank content in the screenshot. Determine whether indexing,
  filtering, or parsing is responsible; distinguish loading, failure, and a
  genuinely empty archive in the UI.
- [ ] **Investigate empty SFX member lists.** Opening `sound/MAIN.SFX` likewise
  shows `0 results` and blank content. Verify the archive index and list its
  supported sound entries, with errors reported instead of appearing empty.
- [ ] **Investigate empty speech archive lists.** Opening `ENGLISH.RTX` shows
  `0 results` and blank content. List the speech index when the archive opens,
  then load only the selected speech resource. Distinguish indexing failure
  from an empty archive.
- [ ] **Check WAV discovery completeness.** The WAV category shows one result,
  `sound/R212.WAV`, with waveform and playback controls. The user expects more.
  Compare the install inventory, manifest, and category filtering; distinguish
  standalone WAV files from sounds inside SFX/RTX archives and CD audio. The
  displayed count alone does not prove files are missing.
- [ ] **Investigate movie loading.** The `disc/INTRO.SMK` screenshot shows
  “Loading selected asset…” with a spinner. Check time to first frame, playback,
  responsiveness, and progress/error reporting. No elapsed time is visible, so
  a permanent hang is not established by this screenshot.

## Presentation

- [ ] **Show scripts and text as readable source.** Present SOUP scripts in DEF
  as scripts rather than only generic data trees. Give INI, LST, and TXT readable
  text views with appropriate formatting. Inspect scripts without executing them.
- [ ] **Improve model inspection lighting.** `fxart/CRAK0009.3D` with
  `fxart/REDCAVE.COL` appears nearly black against the dark background, making
  geometry difficult to inspect. Check lighting, palette, materials, and normals;
  provide useful neutral viewer lighting without silently changing palette data.
  The screenshot does not isolate the cause.
- [ ] **Visualize world geometry and PVO data.** Add useful spatial views based
  on supported preservation semantics. Let users switch between visualization
  and text/JSON views of the decoded data; label unsupported interpretations.

## Implementation constraints and upstream fixes

- [ ] **Preserve runtime, browser-side parsing and lazy loading.** Builds must
  require no game files. The web viewer must work without a parsing server.
  List archives first, index each when opened, and load payloads on selection.
- [ ] **Fix proven preservation defects upstream.** For each issue above,
  distinguish viewer presentation/loading bugs from `rgpre` format or parser
  defects. Where redguard-preservation is responsible, fix it there and create
  a pull request with reproducible evidence and focused tests; reference that
  PR from the viewer change. Do not open speculative fixes based only on these
  screenshots. Keep game files, screenshots, and raw research out of commits.
