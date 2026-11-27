//! Startup and world-entry commands and their live presentation boundary.
use super::*;

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Scene<'w> {
    pub(super) scene_request: ResMut<'w, SceneRequest>,
    pub(super) current: Res<'w, CurrentScene>,
    pub(super) state: Res<'w, State<cyrus_game::AppState>>,
    pub(super) next: ResMut<'w, NextState<cyrus_game::AppState>>,
    pub(super) startup: Option<ResMut<'w, cyrus_startup::StartupPresentation>>,
    pub(super) menu: Option<ResMut<'w, cyrus_startup::MenuState>>,
    pub(super) prepared: Option<Res<'w, cyrus_world::lifecycle::PreparedWorld>>,
    pub(super) ready_scene: Option<Res<'w, cyrus_render::ReadyScene>>,
    pub(super) marker: Option<ResMut<'w, cyrus_script::player::placement::MarkerEntry>>,
    pub(super) clock: Option<Res<'w, cyrus_script::clock::SimulationClock>>,
}

impl Scene<'_> {
    pub(super) fn handle(&mut self, command: Command) -> Outcome {
        let Self {
            scene_request,
            state,
            next,
            startup,
            menu,
            marker,
            ..
        } = self;
        Outcome::Reply(match command {
            Command::LoadScene { .. } if *state.get() == cyrus_game::AppState::SelectingInstall => {
                Err("select and prepare the game files before loading a scene".into())
            }
            Command::LoadScene { name } => {
                scene_request.0 = name.clone();
                if let Some(menu) = menu.as_mut() {
                    menu.entry = None;
                }
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
