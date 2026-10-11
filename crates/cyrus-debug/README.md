# cyrus-debug

The F1 debug menu shared by the native game and browser viewer. It changes
runtime fidelity individually or all at once, an independent flycam toggle and
speed, loads an exported world from a dropdown, and launches a separate
`bevy-inspector-egui` World Inspector. Expand Entities to inspect components,
Resources to edit reflected resources, and Assets to inspect loaded assets.
The inspector stays open independently of the F1 menu and has its own close button.

Press F1, then click **Open inspector**. Click **Close inspector** or the
inspector window's close button to hide it.

The dependency pins the unreleased Bevy 0.20 port from
[upstream PR #324](https://github.com/jakobhellermann/bevy-inspector-egui/pull/324).
Its egui adapter is patched to the same released `bevy_egui` 0.43 used by Cyrus.
Replace the pinned port when an upstream Bevy 0.20 release is available.
Types without registered Bevy reflection cannot be edited by the inspector.

Flycam leaves the player in place. Turning it off returns the camera to the
player pose; it does not change the fidelity settings.

Render distance starts at the installed game's value. Changing it selects
Modern Fog; Original clears the override and returns Fog to Faithful. Modern
Fog defaults to four times the original distance with a gradual distant fade.
Water, Fog, and Sky have independent Faithful/Modern buttons.
