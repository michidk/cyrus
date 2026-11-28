//! Presentation transforms and renderer configuration commands.
use super::*;

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Rendering<'w, 's> {
    pub(super) fidelity: ResMut<'w, Fidelity>,
    pub(super) sky_scroll: Option<ResMut<'w, cyrus_render::environment::SkyScrollSync>>,
    pub(super) day_night: Option<ResMut<'w, cyrus_render::fog::DayNight>>,
    pub(super) modern_effects: Option<ResMut<'w, cyrus_render::fog::ModernEffects>>,
    pub(super) flycam: Option<ResMut<'w, cyrus_render::camera::DetachedView>>,
    pub(super) ray_tracing: Option<ResMut<'w, cyrus_render::fog::RayTracing>>,
    pub(super) camera: Query<
        'w,
        's,
        &'static mut Transform,
        (With<cyrus_render::camera::ViewCamera>, Without<Player>),
    >,
    pub(super) player: Query<'w, 's, &'static mut Transform, With<Player>>,
    pub(super) textures: Option<ResMut<'w, cyrus_render::fog::TextureAnimations>>,
}

impl Rendering<'_, '_> {
    pub(super) fn handle(
        &mut self,
        command: Command,
        commands: &mut Commands,
        scene: &Scene,
    ) -> Outcome {
        match command {
            command @ Command::Screenshot { .. } => self.screenshot(command, commands),
            command @ Command::Teleport { .. } => self.teleport(command),
            command @ Command::Flycam { .. } => self.flycam(command),
            command @ Command::RayTracing { .. } => self.ray_tracing(command),
            command @ Command::SetFidelity { .. } => self.set_fidelity(command),
            command @ Command::SetDayNight { .. } => self.set_day_night(command),
            command @ Command::SetModernEffect { .. } => self.set_modern_effect(command),
            command @ Command::SetSkyScroll { .. } => self.set_sky_scroll(command),
            command @ Command::HoldTextureFrame { .. } => self.hold_texture_frame(command, scene),
            _ => unreachable!("command routed to the wrong rendering handler"),
        }
    }

    fn screenshot(&mut self, command: Command, commands: &mut Commands) -> Outcome {
        Outcome::Reply(match command {
            Command::Screenshot { path } => {
                let path = std::path::PathBuf::from(path);
                match path.parent().map(std::fs::create_dir_all).unwrap_or(Ok(())) {
                    Ok(()) => {
                        commands
                            .spawn(Screenshot::primary_window())
                            .observe(save_to_disk(path.clone()));
                        Ok(json!({"path": path}))
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            _ => unreachable!("command routed to the wrong screenshot handler"),
        })
    }

    fn teleport(&mut self, command: Command) -> Outcome {
        let Self { player, .. } = self;
        Outcome::Reply(match command {
            Command::Teleport { x, y, z } => match player.single_mut() {
                Ok(mut t) => {
                    t.translation = Vec3::new(x, y, z);
                    Ok(json!({"position": [x, y, z]}))
                }
                Err(_) => Err("no player".into()),
            },
            _ => unreachable!("command routed to the wrong teleport handler"),
        })
    }

    fn flycam(&mut self, command: Command) -> Outcome {
        let Self { flycam, camera, .. } = self;
        Outcome::Reply(match command {
            Command::Flycam {
                enabled,
                position,
                look_at,
            } => match flycam.as_deref_mut() {
                Some(flycam) => {
                    flycam.enabled = enabled;
                    if let Ok(mut view) = camera.single_mut() {
                        if let Some(position) = position {
                            view.translation = Vec3::from_array(position);
                        }
                        if let Some(target) = look_at {
                            view.look_at(Vec3::from_array(target), Vec3::Y);
                        }
                    }
                    Ok(json!({"enabled": enabled}))
                }
                None => Err("no flycam".into()),
            },
            _ => unreachable!("command routed to the wrong flycam handler"),
        })
    }

    fn ray_tracing(&mut self, command: Command) -> Outcome {
        let Self {
            fidelity,
            ray_tracing,
            ..
        } = self;
        Outcome::Reply(match command {
            Command::RayTracing { enabled } => match ray_tracing.as_mut() {
                Some(ray_tracing) => {
                    ray_tracing.enabled = enabled;
                    Ok(json!({
                        "enabled": enabled,
                        "traced": cyrus_render::fog::ray_traced(fidelity, ray_tracing),
                        "support": format!("{:?}", ray_tracing.support),
                    }))
                }
                None => Err("this build has no ray tracing (cargo feature `raytracing`)".into()),
            },
            _ => unreachable!("command routed to the wrong ray_tracing handler"),
        })
    }

    fn set_fidelity(&mut self, command: Command) -> Outcome {
        let Self { fidelity, .. } = self;
        Outcome::Reply(match command {
            Command::SetFidelity { area, mode } => {
                match (
                    serde_json::from_value::<Area>(json!(area)),
                    serde_json::from_value::<Mode>(json!(mode)),
                ) {
                    (Ok(area), Ok(mode)) => {
                        fidelity.set(area, mode);
                        Ok(json!(**fidelity))
                    }
                    (Err(error), _) => Err(format!("invalid fidelity area: {error}")),
                    (_, Err(error)) => Err(format!("invalid fidelity mode: {error}")),
                }
            }
            _ => unreachable!("command routed to the wrong set_fidelity handler"),
        })
    }

    fn set_day_night(&mut self, command: Command) -> Outcome {
        let Self { day_night, .. } = self;
        Outcome::Reply(match command {
            Command::SetDayNight { hour, running } => match (day_night.as_deref_mut(), hour) {
                (None, _) => Err("modern lighting is unavailable".into()),
                (Some(_), Some(hour)) if !(0.0..=24.0).contains(&hour) => {
                    Err("hour must be within 0 to 24".into())
                }
                (Some(cycle), hour) => {
                    cycle.enabled = hour.is_some();
                    cycle.hour = hour.unwrap_or(cycle.hour);
                    cycle.running = running;
                    Ok(
                        json!({"enabled": cycle.enabled, "hour": cycle.hour, "running": cycle.running}),
                    )
                }
            },
            _ => unreachable!("command routed to the wrong set_day_night handler"),
        })
    }

    fn set_modern_effect(&mut self, command: Command) -> Outcome {
        let Self { modern_effects, .. } = self;
        Outcome::Reply(match command {
            Command::SetModernEffect { effect, enabled } => {
                match (
                            modern_effects.as_deref_mut(),
                            serde_json::from_value::<cyrus_render::fog::Effect>(json!(effect)),
                        ) {
                            (None, _) => Err("modern lighting is unavailable".into()),
                            (Some(_), Err(_)) => Err(
                                "effect must be shadows|ambient_occlusion|contact_shadows|anti_aliasing|bloom|object_lights|water_gloss|grading|auto_exposure|god_rays|lens_flare|sky|water_reflections|depth_of_field|motion_blur"
                                    .into(),
                            ),
                            (Some(effects), Ok(effect)) => {
                                *effects.switch(effect) = enabled;
                                Ok(json!(*effects))
                            }
                        }
            }
            _ => unreachable!("command routed to the wrong set_modern_effect handler"),
        })
    }

    fn set_sky_scroll(&mut self, command: Command) -> Outcome {
        let Self { sky_scroll, .. } = self;
        Outcome::Reply(match command {
            Command::SetSkyScroll { direction, offset } => match sky_scroll.as_deref_mut() {
                Some(reference) => reference
                    .set(direction, offset)
                    .map(|()| json!({"direction": direction, "offset": offset}))
                    .map_err(|error| error.to_string()),
                None => Err("sky renderer is unavailable".into()),
            },
            _ => unreachable!("command routed to the wrong set_sky_scroll handler"),
        })
    }

    fn hold_texture_frame(&mut self, command: Command, scene: &Scene) -> Outcome {
        let Self { textures, .. } = self;
        let Scene {
            state,
            scene_request,
            ready_scene,
            prepared,
            ..
        } = scene;
        Outcome::Reply(match command {
            Command::HoldTextureFrame {
                world,
                image,
                frame,
            } => {
                if *state.get() != cyrus_game::AppState::InGame
                    || world != scene_request.0
                    || !ready_scene.as_ref().is_some_and(|ready| {
                        prepared
                            .as_ref()
                            .is_some_and(|world| ready.0 == world.animations)
                    })
                {
                    Err("texture capture requires the requested ready world".into())
                } else if let Some(textures) = textures.as_deref_mut() {
                    textures
                        .hold(&image, frame)
                        .map(|()| json!({"textures": textures.snapshot()}))
                        .map_err(|error| error.to_string())
                } else {
                    Err("texture renderer is unavailable".into())
                }
            }
            _ => unreachable!("command routed to the wrong hold_texture_frame handler"),
        })
    }
}
