//! Original world owners are assembled on an unpublished replacement.
use super::*;
use rgpre::import::savegame::{
    SaveGame, SaveModelAllocation, parse_save_camera_globals, parse_save_cameras,
    parse_save_code_owners, parse_save_families, parse_save_objects, parse_save_ropes,
    parse_save_script_locals, parse_save_shells,
};

/// Independently resolved installed allocations for one saved world.
/// Neither serialized handles nor saved traversal order may construct these tables.
#[derive(Clone, Copy)]
pub struct OriginalWorldAllocations<'a> {
    pub models: &'a [SaveModelAllocation],
    pub walk_maps: &'a [u32],
    pub ropes: &'a [u32],
    /// Replacement sample slots after original-load cleanup, including stopped slots
    /// whose owning native continuation still retains a sample number.
    pub ambient_channels: &'a BTreeMap<u8, u8>,
}

impl ScriptWorld {
    /// Compose retained native world owners without publishing intermediate state.
    ///
    /// Ownership of the unpublished world is consumed: failure cannot expose its
    /// earlier component replacements. The enclosing session must also supply its
    /// saved globals, HUD, clock, visited worlds and host services before publication.
    /// No initialization callback, script instruction or service update runs here.
    pub fn with_original_world(
        mut self,
        saved: &SaveGame,
        sources: &WorldSnapshotSources<'_>,
        allocations: OriginalWorldAllocations<'_>,
        session_globals: &[u8],
    ) -> Result<Self> {
        anyhow::ensure!(
            self.world == saved.metadata.world_id,
            "world {} original composition: saved world {} differs",
            self.world,
            saved.metadata.world_id
        );
        let section = |name: [u8; 4]| -> Result<&[u8]> {
            let mut sections = saved.sections.iter().filter(|section| section.name == name);
            let bytes = &sections
                .next()
                .with_context(|| {
                    format!(
                        "world {} original composition: {} absent",
                        self.world,
                        String::from_utf8_lossy(&name)
                    )
                })?
                .data;
            anyhow::ensure!(
                sections.next().is_none(),
                "duplicate original section {}",
                String::from_utf8_lossy(&name)
            );
            Ok(bytes)
        };
        let objects = parse_save_objects(section(*b"SMOB")?)?;
        let codes = parse_save_code_owners(section(*b"SMCO")?)?;
        let shells = parse_save_shells(section(*b"SMSH")?)?;
        let locals = parse_save_script_locals(section(*b"SVRG")?)?;
        let cameras = parse_save_cameras(section(*b"SVCM")?)?;
        let camera_globals = parse_save_camera_globals(section(*b"SVGV")?, session_globals)?;
        let families = parse_save_families(section(*b"SMFM")?)?;
        let ropes = parse_save_ropes(section(*b"SMRP")?)?;
        let bindings = objects
            .iter()
            .map(|object| -> Result<_> {
                let mut receivers =
                    self.actors
                        .iter()
                        .enumerate()
                        .filter(|(_, actor)| match &actor.source {
                            ScriptActorSource::Player => object.object_id == -2,
                            ScriptActorSource::AlternatePlayer => object.object_id == -3,
                            ScriptActorSource::Camera => object.object_id == 2,
                            ScriptActorSource::Map(placement)
                                if !placement.object_type.has_separate_record() =>
                            {
                                object.object_id >= 0
                                    && actor
                                        .runtime_identifier
                                        .is_some_and(|id| i32::from(id as u16) == object.object_id)
                            }
                            _ => false,
                        });
                let (index, _) = receivers.next().with_context(|| {
                    format!(
                        "world {} original actor {}: installed owner absent",
                        self.world, object.object_id
                    )
                })?;
                anyhow::ensure!(
                    receivers.next().is_none(),
                    "original actor {} has ambiguous installed owners",
                    object.object_id
                );
                Ok((self.id(index)?, object))
            })
            .collect::<Result<Vec<_>>>()?;
        // Installed placements retain MPOB order; the two standalone player models
        // construct afterward, selected role first, without issuing animation events.
        let mut combat_order = Vec::new();
        for (index, actor) in self.actors.iter().enumerate() {
            if let ScriptActorSource::Map(placement) = &actor.source
                && placement.object_type == PlacementType::MODEL
                && sources.animations.actor(actor.name())?["model_load_path"].as_str()
                    == Some("animation_table")
            {
                combat_order.push(self.id(index)?);
            }
        }
        for alternate in [false, true] {
            for (index, actor) in self.actors.iter().enumerate() {
                if matches!(
                    (&actor.source, alternate),
                    (ScriptActorSource::Player, false) | (ScriptActorSource::AlternatePlayer, true)
                ) {
                    combat_order.push(self.id(index)?);
                }
            }
        }
        self.prepare_original_combat_allocations(sources.animations, &combat_order)?;
        self.restore_original_actor_states(&bindings)?;
        self.restore_original_combat(&bindings)?;
        let animated = bindings
            .iter()
            .copied()
            .filter(|(_, object)| object.animation_header.is_some())
            .collect::<Vec<_>>();
        self.restore_original_bound_animations(&animated, allocations.models, sources.animations)?;
        self.restore_original_model_presentations(&bindings, sources.animations)?;
        self.restore_original_presentation(&bindings)?;
        self.restore_original_families(&bindings, &families)?;
        self.restore_original_ropes(allocations.ropes, &ropes, &objects)?;
        self.restore_original_camera(&bindings, &cameras, &camera_globals)?;
        self.globals.dialogue = Some(super::super::DialogueState::from_original_load(
            &sources.scripts.dialogue,
        )?);
        // Execution is deliberately last: a bad shell cursor after all preceding
        // service preparation still cannot publish any part of this replacement.
        self.restore_original_execution(
            OriginalExecutionOwners {
                actors: &objects,
                codes: &codes,
                shells: &shells,
                locals: &locals,
                walk_map_ids: allocations.walk_maps,
                ambient_channels: allocations.ambient_channels,
            },
            sources.scripts,
            sources.animations,
        )?;
        Ok(self)
    }
}

impl ScriptWorld {
    fn restore_original_model_presentations(
        &mut self,
        bindings: &[(ScriptActorId, &rgpre::import::savegame::SaveObject)],
        data: &Animations,
    ) -> Result<()> {
        for &(id, object) in bindings {
            let index = self.resolve(id)?;
            let actor = &self.actors[index];
            let animation = if let Some(reference) = self.animation_frame(id)? {
                Some(if actor.placement().is_some() {
                    data.resolve_map_frame(index, reference)?
                } else {
                    data.resolve_frame(actor.name(), reference)?
                })
            } else {
                None
            };
            let radius = if let Some(frame) = &animation {
                Some(
                    data.models
                        .get(&frame.model)
                        .context("original installed geometry absent")?
                        .radius
                        .context("original installed geometry radius absent")?,
                )
            } else {
                actor.model.static_radius
            };
            let transform = object.actor_state()?.model_transform;
            let mut pose = self.spatial(id)?.local;
            pose.matrix = cyrus_world::transform::Matrix(transform.matrix());
            let actor = &mut self.actors[index];
            if animation.is_some() {
                actor.model.animation_radius = radius;
            }
            actor.model.current = radius.map(|radius| model::ModelPresentation {
                pose,
                position: transform.translation(),
                radius,
                animation,
            });
        }
        Ok(())
    }
}
