//! Startup and world-entry commands and their live presentation boundary.
use super::*;

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Scene<'w> {
    pub(super) scene_request: ResMut<'w, SceneRequest>,
    pub(super) current: Res<'w, CurrentScene>,
    pub(super) state: Res<'w, State<cyrus_game::AppState>>,
    pub(super) next: ResMut<'w, NextState<cyrus_game::AppState>>,
    pub(super) startup: Option<ResMut<'w, cyrus_startup::StartupPresentation>>,
    pub(super) entry: Option<Res<'w, cyrus_startup::NewGameEntry>>,
    pub(super) menu: Option<ResMut<'w, cyrus_startup::MenuState>>,
    pub(super) prepared: Option<Res<'w, cyrus_world::lifecycle::PreparedWorld>>,
    pub(super) ready_scene: Option<Res<'w, cyrus_render::ReadyScene>>,
    pub(super) marker: Option<ResMut<'w, cyrus_script::player::placement::MarkerEntry>>,
    pub(super) clock: Option<ResMut<'w, cyrus_script::clock::SimulationClock>>,
}

impl Scene<'_> {
    /// Readiness is bound to the current world and its prepared animation asset.
    pub(super) fn is_ready_world(&self, world: u32) -> bool {
        *self.state.get() == cyrus_game::AppState::InGame
            && self.current.world == Some(world)
            && self.prepared.as_deref().is_some_and(|prepared| {
                prepared.world == Some(world)
                    && self
                        .ready_scene
                        .as_deref()
                        .is_some_and(|ready| ready.0 == prepared.animations)
            })
    }

    pub(super) fn handle(&mut self, command: Command, commands: &mut Commands) -> Outcome {
        let Self {
            scene_request,
            state,
            next,
            startup,
            entry,
            marker,
            ..
        } = self;
        Outcome::Reply(match command {
            Command::LoadScene { .. } if *state.get() == cyrus_game::AppState::SelectingInstall => {
                Err("select and prepare the game files before loading a scene".into())
            }
            Command::LoadScene { name } => {
                scene_request.0 = name.clone();
                *entry = None;
                commands.remove_resource::<cyrus_startup::NewGameEntry>();
                // A world entered by name starts at its default marker, not a retained one.
                if let Some(marker) = marker.as_mut() {
                    **marker = Default::default();
                }
                if *state.get() != cyrus_game::AppState::InGame {
                    next.set(cyrus_game::AppState::InGame);
                }
                Ok(json!({"scene": name}))
            }
            Command::SkipStartup => {
                if startup.is_some() && *state.get() != cyrus_game::AppState::InGame {
                    if let Some(startup) = startup.as_mut() {
                        startup.skip = true;
                    }
                    Ok(json!({"state": format!("{:?}", state.get()), "destination": "Menu"}))
                } else {
                    Err("startup presentation is not active".into())
                }
            }
            _ => unreachable!("command routed to the wrong scene handler"),
        })
    }
}
