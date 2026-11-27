//! Read-only aggregate control-state publication.
use super::*;

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Inspection<'w> {
    pub(super) preloaded: Option<Res<'w, Preloaded>>,
    pub(super) install: Option<Res<'w, cyrus_assets::install::GameInstall>>,
    pub(super) hud: Option<Res<'w, cyrus_render::hud::HudState>>,
    pub(super) logbook: Option<Res<'w, cyrus_script::logbook::LogbookScreen>>,
    pub(super) inventory_screen: Option<Res<'w, cyrus_script::inventory_screen::InventoryScreen>>,
    pub(super) examine: Option<Res<'w, cyrus_script::item_viewer::ExamineScreen>>,
    pub(super) island_map: Option<Res<'w, cyrus_script::island_map::IslandMapScreen>>,
    pub(super) scripts: Option<Res<'w, cyrus_script::script::ScriptStatus>>,
    pub(super) console: Res<'w, cyrus_console::Console>,
    pub(super) input: Res<'w, cyrus_script::player::input::PlayerInput>,
}

impl Inspection<'_> {
    pub(super) fn state(
        &self,
        scene: &Scene,
        rendering: &Rendering,
        gameplay: &Gameplay,
        audio: &Audio,
        acquisition: &Acquisition,
    ) -> Result<serde_json::Value, String> {
        let Self {
            preloaded,
            hud,
            logbook,
            inventory_screen,
            examine,
            island_map,
            console,
            input,
            ..
        } = self;
        let Scene {
            state,
            current,
            startup,
            menu,
            entry,
            prepared,
            ready_scene,
            clock,
            marker,
            ..
        } = scene;
        let Rendering {
            player,
            textures,
            fidelity,
            flycam,
            camera,
            ..
        } = rendering;
        let Gameplay {
            random,
            cheats,
            script_hud,
            ..
        } = gameplay;
        let Acquisition {
            frames,
            effect_geometry_pending,
            rendered_actor_pending,
            ..
        } = acquisition;
        let scripts = self.script_state(gameplay);
        let scene_ready = ready_scene.as_ref().is_some_and(|ready| {
            prepared
                .as_ref()
                .is_some_and(|world| ready.0 == world.animations)
        });
        let transform = player.single().ok().map(|t| {
            let (yaw, pitch, _) = t.rotation.to_euler(EulerRot::YXZ);
            json!({"position": t.translation.to_array(), "yaw": yaw, "pitch": pitch})
        });
        let preload = preloaded.as_ref().map(|p| {
                            json!({"parsed": p.parsed, "failed": p.failed.len(), "by_type": p.by_type, "unsupported": p.unsupported, "report": cyrus_config::config_dir().join("preload-report.json").display().to_string()})
                        });
        let entry = entry.as_deref().copied().filter(|entry| {
            scene_ready
                && *state.get() == cyrus_game::AppState::InGame
                && current.world == Some(entry.world)
                && prepared
                    .as_ref()
                    .is_some_and(|world| world.world == Some(entry.world))
                && gameplay.script_actors.as_ref().is_some_and(|actors| {
                    !actors.current_is_restored()
                        && actors
                            .current
                            .as_ref()
                            .is_some_and(|world| world.world == entry.world)
                })
                && scripts.as_ref().and_then(|s| s.get("world")) == Some(&json!(entry.world))
                && marker
                    .as_ref()
                    .is_some_and(|marker| marker.marker == entry.marker)
        });
        let entry = match (entry, self.install.as_ref()) {
            (Some(entry), Some(install)) => {
                let options = cyrus_assets::scene::world_options(install).map_err(|error| {
                    format!(
                        "native entry world {} in {}: {error:#}",
                        entry.world,
                        install.root.display()
                    )
                })?;
                let scene = options
                    .iter()
                    .find(|[id, _]| id == &entry.world.to_string())
                    .ok_or_else(|| {
                        format!(
                            "native entry world {} absent from installed WORLD.INI in {}",
                            entry.world,
                            install.root.display()
                        )
                    })?;
                Some(json!({"route": entry.route, "world": entry.world,
                    "marker": entry.marker, "scene": scene[1]}))
            }
            _ => None,
        };
        Ok(
            json!({"entry": entry, "random_state": random.state(), "effect_geometry_pending": effect_geometry_pending.is_some(), "rendered_actor_pending": rendered_actor_pending.is_some(), "textures": textures.as_ref().map(|textures| textures.snapshot()), "state": format!("{:?}", state.get()), "menu": menu.as_deref(), "startup": startup.as_deref(), "hud": hud.as_deref(), "logbook": logbook.as_deref(), "inventory_screen": inventory_screen.as_deref(), "examine": examine.as_deref(), "island_map": island_map.as_deref(), "script_hud": script_hud.as_ref().map(|hud| hud.inspect()), "cheats": cheats.inspect(), "console": **console, "scripts": scripts, "scene": current.name, "scene_ready": scene_ready, "player": transform, "fidelity": **fidelity, "frame": frames.0, "clock": clock.as_ref().map(|clock| json!({"game_seconds": clock.game_seconds(), "fps": clock.fps(), "continuation": clock.snapshot()})), "preload": preload, "player_input": **input, "music_track": audio.music.as_ref().and_then(|m| m.track), "music_paused": audio.music_sink.iter().next().map(bevy::audio::AudioSinkPlayback::is_paused), "sounds": json!({"playing": audio.playing.iter().count(), "paused": audio.playing.iter().flatten().filter(|sink| bevy::audio::AudioSinkPlayback::is_paused(*sink)).count()}), "flycam": {"enabled": flycam.as_ref().is_some_and(|f| f.enabled), "position": camera.single().ok().map(|t| t.translation.to_array()), "rotation": camera.single().ok().map(|t| t.rotation.to_array())}}),
        )
    }
    fn script_state(&self, gameplay: &Gameplay) -> Option<serde_json::Value> {
        // Preparation status is sampled before ordinary simulation. Report
        // completed-update observations from the live registry at this boundary.
        let mut scripts = self.scripts.as_deref().map(|status| json!(status));
        if let (Some(status), Some(actors)) = (scripts.as_mut(), gameplay.script_actors.as_deref())
            && let Some(world) = actors.current.as_ref()
            && status.get("world") == Some(&json!(world.world))
        {
            status["ordinary_updates"] = json!(world.ordinary_updates());
            status["restored"] = json!(actors.current_is_restored());
        }
        scripts
    }
}
