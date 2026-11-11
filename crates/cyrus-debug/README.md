# cyrus-debug

The F1 debug menu shared by the native and browser game. It changes
runtime fidelity individually or all at once, loads an installed world from a dropdown, and launches a separate
`bevy-inspector-egui` World Inspector. Expand Entities to inspect components,
Resources to edit reflected resources, and Assets to inspect loaded assets.
The inspector scrolls within its resized window. World choices show their installed
map name alongside the WORLD.INI ID, including shared-map palette variants.
The inspector stays open independently of the F1 menu and has its own close button.
F1 is available during startup, at the book menu, and in a world. Its overlay
camera persists when startup and world cameras are replaced. Headless launches
do not register the debug UI.

Press F1, then click **Open inspector**. Click **Close inspector** or the
inspector window's close button to hide it.

Press F1, then click **Open performance** for a Performance window. It plots the
frame rate and the frame time of every real-time frame over the last 30 seconds
(newest on the right, value axis from zero to the next round number), with the
average, 1% low (the slowest 1% of frames) and worst frame below the plots. Beneath
that it lists the GPU adapter, window resolution, entity count, visible and total
meshes, lights, and loaded mesh and image assets. Frames are recorded whether or not
the window is open, so the plots are already filled in when it opens; the counts are
only gathered while it is open. Like the inspector, the window stays open
independently of the F1 menu and does not capture game input.

Press F1, then click **Open day/night** for the modern lighting's time of day:
turn the cycle on, pick the hour (12:00 is the world's installed sun) or jump to
dawn, noon, dusk or night, and let it run at a chosen speed. It needs Modern
lighting, and the panel offers to switch it. The browser build has no modern
lighting and says so.

Press F1, then click **Open effects** for the modern lighting's optional effects: a switch
for each one (tonemapping and colour grading, auto exposure, god rays, lens flare and sun
glare, sky glow and stars, water reflections, swaying plants, depth of field, motion blur),
with the tonemapper, exposure, saturation, contrast, haze density and aperture beneath them.
They need Modern lighting, and the panel offers to switch it.

The dependency pins the unreleased Bevy 0.20 port from
[upstream PR #324](https://github.com/jakobhellermann/bevy-inspector-egui/pull/324).
Its egui adapter is patched to the same released `bevy_egui` 0.43 used by Cyrus.
Replace the pinned port when an upstream Bevy 0.20 release is available.
Types without registered Bevy reflection cannot be edited by the inspector.

Hosts populate `WorldPicker` and consume its pending selection; this crate does
not read the install or browser DOM. A world selection enters through the game's
loading screen and player-entry lifecycle on native and browser hosts. It clears
the inspection camera and enters the selected world at marker zero.

The menu has no camera controls: the view is detached only by an explicit inspection view
or the control protocol, and free flight is the `flycam` mod (F11). Neither moves the
player nor changes the fidelity settings.

Render distance starts at the installed game's value. Changing it selects
Modern Fog; Original clears the override and returns Fog to Faithful. Modern Fog
also expands terrain and water submission to the selected distance. Modern Fog
defaults to 50,000 engine units, or the original distance where that is farther,
with a gradual distant fade.
Fog and Sky have independent Faithful/Modern buttons. Water changes only through native mods;
`cyrus-mods` adds its Mods window while this menu is open.

F1's **Anisotropic filtering (up to 16×)** checkbox independently filters world
textures at oblique angles. It defaults off and works with either lighting mode.
**All Faithful** turns it off; **All modern** turns it on.
