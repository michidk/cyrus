//! Optional one-line speech advancement. The host owns audio teardown and script continuation.
use cyrus_mod_sdk::{Cx, Key, Mod};
struct DialogueSkip;
impl Mod for DialogueSkip {
    const ID: &'static str = "dialogue-skip";
    fn frame(cx: &Cx) {
        let key = match cx.settings.choice("key").as_str() {
            "Space" => Key::SPACE,
            _ => Key::code("Backspace"),
        };
        if cx.input.bound(key) {
            if cx.dialogue.available() {
                cx.hud.show("Dialogue skip key is assigned to a game action. Choose another key in Mods settings.");
            }
            return;
        }
        if cx.dialogue.available() && cx.input.tapped(key) {
            cx.dialogue.advance();
        }
    }
}
cyrus_mod_sdk::export_mod!(DialogueSkip);
