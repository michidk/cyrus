//! Shows the nearest visible actor in the corner: its script name, health and distance in map
//! units. An example of the read-only `actors` hook; the distance filter reads positions only,
//! so the per-actor calls stay cheap even in a world with a thousand actors.
use cyrus_mod_sdk::{Cx, Mod};

/// Actors farther than this, in map units, are not shown.
const RANGE: f32 = 1500.0;

struct ActorInfo;

impl Mod for ActorInfo {
    const ID: &'static str = "actor-info";

    fn frame(cx: &Cx) {
        let Some(player) = cx.player.position() else {
            return;
        };
        let nearest = cx.actors.nearest(player, RANGE, |actor| {
            // The player is an actor too, and a hidden one is not there to see.
            !actor.hidden && !actor.name.eq_ignore_ascii_case("CYRUS")
        });
        match nearest {
            Some((actor, distance)) => cx.hud.show(&format!(
                "Nearest: {} (health {}) {distance:.0} away",
                actor.name, actor.health
            )),
            None => cx.hud.show("Nearest: nothing in range"),
        }
    }
}

cyrus_mod_sdk::export_mod!(ActorInfo);
