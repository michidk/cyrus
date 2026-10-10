# cyrus-debug

The F1 debug menu shared by the native game and browser viewer. It changes
runtime fidelity individually or all at once, an independent flycam toggle and
speed, loads an exported world from a dropdown, and launches a separate entity
transform inspector window. The full `bevy-inspector-egui` plugin cannot be used
until it supports Bevy 0.20.

Flycam leaves the player in place. Turning it off returns the camera to the
player pose; it does not change the fidelity settings.
