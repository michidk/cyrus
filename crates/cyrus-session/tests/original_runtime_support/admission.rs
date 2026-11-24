use super::*;

fn assert_first_native_update(app: &mut App, scripts: &Scripts) -> Result<()> {
    let world = app
        .world_mut()
        .resource_mut::<ScriptActors>()
        .into_inner()
        .current
        .as_mut()
        .unwrap();
    let programs = scripts
        .actors
        .values()
        .map(|actor| {
            (
                cyrus_soup::CodeOrigin(actor.code_origin),
                std::sync::Arc::new(actor.program.clone()),
            )
        })
        .collect();
    let waiting = world
        .actors()
        .iter()
        .enumerate()
        .find_map(|(index, actor)| {
            let runtime = actor.runtime()?;
            let vm = runtime.snapshot(&programs).ok()?;
            let calls = vm.task_calls().ok()?;
            (calls.len() == 1
                && calls[0].call.function == cyrus_soup::Native::Wait
                && runtime.task_slots()[0].4 < u16::MAX)
                .then_some(index)
        })
        .context("saved native Wait absent")?;
    let cursor = world.actors()[waiting]
        .runtime()
        .unwrap()
        .program_counter(cyrus_soup::Thread::Main);
    world.advance(
        world.id(waiting)?,
        ScriptStage::Tasks,
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        },
        Some(&mut |_, _| Ok(())),
        None,
        100,
    )?;
    assert_eq!(
        world.actors()[waiting]
            .runtime()
            .unwrap()
            .program_counter(cyrus_soup::Thread::Main),
        cursor
    );
    Ok(())
}

pub struct AdmissionCompletion<'a> {
    pub sources: &'a BTreeMap<u32, WorldSnapshotSources<'a>>,
    pub hud: &'a HudAsset,
    pub preferences: &'a Preferences,
    pub scene: &'a bevy::prelude::Handle<PlayerScene>,
    pub animation: &'a bevy::prelude::Handle<Animations>,
    pub retained: &'a SessionWorldAssets,
}
impl AdmissionCompletion<'_> {
    pub fn verify(
        &self,
        app: &mut App,
        snapshot: RuntimeSessionSnapshot,
        prepare: impl Fn(RuntimeSessionSnapshot) -> Result<OriginalRuntimeAdmission>,
    ) -> Result<()> {
        let before = serde_json::to_value(&snapshot)?;
        self.assert_late_rejection(
            prepare(snapshot.clone()).context("initial original preparation")?,
            app,
            &before,
        )?;
        self.publish(
            prepare(snapshot).context("public original preparation")?,
            app,
        )
        .context("public original publication")?;
        assert_projection(app)?;
        let checkpoint = RuntimeSessionSnapshot::capture(app.world(), self.sources, self.hud)?;
        checkpoint
            .clone()
            .plan(LogicalSessionRestoreInputs {
                sources: self.sources,
                last_generation: 100,
                hud: self.hud,
                preferences: self.preferences,
                bios_tick: 100,
            })
            .context("published checkpoint reconstruction")?;
        let active_before = serde_json::to_value(&checkpoint)?;
        self.assert_late_rejection(
            prepare(checkpoint).context("active original preparation")?,
            app,
            &active_before,
        )?;
        let id = app
            .world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .world;
        assert_first_native_update(app, self.sources[&id].scripts)?;
        eprintln!(
            "runtime admitted world {id} through first native update without callback reissue"
        );
        Ok(())
    }
    fn publish(&self, admission: OriginalRuntimeAdmission, app: &mut App) -> Result<()> {
        let hud_handle =
            app.world_mut()
                .resource_mut::<Assets<HudAsset>>()
                .add(serde_json::from_slice::<HudAsset>(&serde_json::to_vec(
                    self.hud,
                )?)?);
        app.add_plugins(SessionBoundaryPlugin);
        // The first presentation pass must see the complete replacement, never the old host.
        app.add_systems(
            bevy::prelude::PostUpdate,
            assert_published_at_pose_render.in_set(cyrus_render::animation::PoseRender),
        );
        app.world_mut()
            .resource_mut::<SessionBoundary>()
            .import_original_session(
                admission,
                SessionLoadAssets {
                    sources: self.retained.clone(),
                    hud: hud_handle,
                    hud_images: None,
                    samples: BTreeMap::new(),
                    dialogue: BTreeMap::new(),
                    music: None,
                },
            )?;
        app.world_mut().run_schedule(bevy::app::PostUpdate);
        let result = app
            .world_mut()
            .resource_mut::<SessionBoundary>()
            .take_result()
            .context("original runtime publication did not finish")??;
        ensure!(
            matches!(result, SessionBoundaryResult::Committed),
            "original runtime returned capture"
        );
        Ok(())
    }
    fn assert_late_rejection(
        &self,
        admission: OriginalRuntimeAdmission,
        app: &mut App,
        before: &serde_json::Value,
    ) -> Result<()> {
        let stage = SessionEntityStage::prepare(admission.snapshot(), None, app.world_mut())?;
        let child = app
            .world_mut()
            .spawn((
                Transform::default(),
                bevy::prelude::GlobalTransform::default(),
                bevy::prelude::ChildOf(stage.player()),
            ))
            .id();
        let candidate = self.finish(admission, &stage, app)?;
        app.world_mut().despawn(child);
        let error = candidate
            .commit(app.world_mut())
            .err()
            .context("late projection target disappearance admitted")?;
        ensure!(
            error.to_string().contains("projection target"),
            "rejected before late projection validation: {error:#}"
        );
        assert_eq!(
            &serde_json::to_value(RuntimeSessionSnapshot::capture(
                app.world(),
                self.sources,
                self.hud
            )?)?,
            before
        );
        stage.discard(app.world_mut());
        Ok(())
    }
    fn finish(
        &self,
        admission: OriginalRuntimeAdmission,
        stage: &SessionEntityStage,
        app: &App,
    ) -> Result<RuntimeSessionCandidate> {
        let replacement = stage.player();
        admission
            .plan(LogicalSessionRestoreInputs {
                sources: self.sources,
                last_generation: 20,
                hud: self.hud,
                preferences: self.preferences,
                bios_tick: 100,
            })?
            .finish(
                PlayerReplacement {
                    entity: replacement,
                    scene: self.scene.clone(),
                    animation: self.animation.clone(),
                },
                SessionServices {
                    sources: self.retained.clone(),
                    scene: None,
                    camera_entity: None,
                    entities: stage.bindings(),
                    source_audio: &BTreeMap::new(),
                    dialogue: RestoredDialogueAssets {
                        server: app.world().resource::<AssetServer>(),
                        audio_sources: app.world().resource::<Assets<AudioSource>>(),
                        loaded: app.world().resource::<Assets<DialogueClip>>(),
                        clips: &BTreeMap::new(),
                        requests: &BTreeMap::new(),
                        hud: None,
                    },
                    music_source: None,
                    hud_asset: self.hud,
                    hud_images: None,
                    sky_images: None,
                    render_playback: None,
                    player_model_root: None,
                    actor_model_roots: &BTreeMap::new(),
                    environments: &BTreeMap::new(),
                },
                app.world(),
            )
    }
}

fn assert_projection(app: &mut App) -> Result<()> {
    let replacement = app
        .world_mut()
        .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<Player>>()
        .single(app.world())?;
    assert!(matches!(
        app.world().get::<PlayerEntrySupport>(replacement),
        Some(PlayerEntrySupport::RestoredContinuation)
    ));
    let published = app.world().resource::<ScriptActors>();
    let current = published.current.as_ref().unwrap();
    let player_id = current
        .selected_player()
        .context("published player absent")?;
    let model = current
        .model_presentation(player_id)?
        .context("published player model absent")?;
    assert_eq!(
        app.world()
            .get::<bevy::prelude::GlobalTransform>(replacement)
            .context("first rendered affine absent")?
            .to_matrix(),
        bevy::prelude::Mat4::from(cyrus_render::script_transform::scene_transform_at(
            model.pose,
            model.position
        ))
    );
    Ok(())
}
fn assert_published_at_pose_render(world: &mut bevy::prelude::World) {
    assert!(world.contains_resource::<SessionPublicationBoundary>());
    let player = world
        .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<Player>>()
        .single(world)
        .unwrap();
    assert!(matches!(
        world.get::<PlayerEntrySupport>(player),
        Some(PlayerEntrySupport::RestoredContinuation)
    ));
}
