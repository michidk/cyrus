//! Automatic late collision composes retained candidates and logical actor response.
use super::{ScriptActorId, ScriptWorld};
use anyhow::{Context, Result, ensure};
use cyrus_collision::ground::SelectedSupport;

pub(super) struct OrdinaryLateInputs {
    pub ticks: u32,
    pub weapon: Option<super::weapon::WeaponInputs>,
}

impl ScriptWorld {
    fn assign_ordinary_support_surface(
        &mut self,
        id: ScriptActorId,
        scene: &crate::player::data::PlayerScene,
        support: Option<&SelectedSupport>,
    ) -> Result<()> {
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("support surface control unavailable")?;
        if control.queued_motion != 0
            || control.directional_state.iter().any(|flags| flags & 3 != 0)
        {
            return Ok(());
        }
        if control.falling_state != 0
            || control.sliding_state != 0
            || (27..=29).contains(&control.living_state)
        {
            return Ok(());
        }
        let surface = support
            .map(|support| {
                scene.support_surface(
                    support,
                    self.spatial(id)?.query_pose()?.position.map(i64::from),
                )
            })
            .transpose()?
            .unwrap_or(0);
        self.set_actor_surface(id, surface)?;
        self.surface_veto = Some(matches!(surface, 5 | 11));
        Ok(())
    }

    pub(super) fn resolve_ordinary_late_collision(
        &mut self,
        id: ScriptActorId,
        query: super::ground::OrdinaryGroundQuery<'_>,
        inputs: OrdinaryLateInputs,
        rollback: &super::recovery::OrdinaryRollbackSnapshot,
        recovery: super::recovery::OrdinaryStableRecoveryInputs<'_>,
        sounds: &mut dyn FnMut(ScriptActorId, super::super::ScriptSound) -> Result<()>,
    ) -> Result<()> {
        let OrdinaryLateInputs { ticks, weapon } = inputs;
        let environment = super::transition::ObstacleEnvironment {
            selected_player: weapon.map(|input| input.selected_player),
            auto_grab: false,
            item_system_ready: weapon.is_some_and(|input| input.item_system_ready),
            shell_execution: weapon.is_some_and(|input| input.shell_execution),
            ticks,
        };
        let obstacle_query = || super::obstacle::OrdinaryObstacleQuery {
            scene: query.scene,
            animations: query.animations,
            prepared: query.prepared,
            vertical_offset: 0,
            respond: true,
        };
        let mask = self.actors[id.index].attributes[1];
        let control = self.actors[id.index]
            .control
            .as_ref()
            .context("late collision control unavailable")?;
        let special = (10..=19).contains(&control.living_state);
        ensure!(
            control.falling_state == 0,
            "falling support probe distance owner unavailable"
        );
        if !special {
            self.resolve_ordinary_obstacle_wrapper(id, obstacle_query(), environment, sounds)?;
        }
        if mask & 8 != 0 {
            self.resolve_ordinary_overlap_stage(id, obstacle_query(), environment, sounds)?;
        }
        let ground_query = || super::ground::OrdinaryGroundQuery {
            scene: query.scene,
            animations: query.animations,
            prepared: query.prepared,
            distance: query.distance,
            sidedness: query.sidedness,
        };
        let prevalidation = self.query_ordinary_ground_prevalidation(id, ground_query(), false)?;
        self.apply_ordinary_ground_prevalidation(
            id,
            prevalidation,
            query.animations,
            environment,
            sounds,
        )?;
        let support = self.query_ordinary_ground(id, ground_query())?;
        if mask & 4 != 0 {
            self.assign_ordinary_support_surface(id, query.scene, support.as_ref())?;
            let living_state = self.actors[id.index]
                .control
                .as_ref()
                .expect("admitted ground control")
                .living_state;
            if matches!(living_state, 20 | 21) {
                self.admit_actor_weapon_draw(
                    id,
                    weapon.context("combat ground weapon admission unavailable")?,
                    ticks,
                    sounds,
                )?;
            } else if !special {
                let actor = &self.actors[id.index];
                let control = actor.control.as_ref().expect("admitted late control");
                ensure!(
                    !matches!(control.living_state, 27..=32),
                    "state-specific ground response owner unavailable"
                );
                ensure!(
                    control.queued_motion == 0 && control.falling_state == 0,
                    "airborne ground response timing and terrain veto unavailable"
                );
                if let Some(support) = support.as_ref() {
                    ensure!(
                        control.sliding_state == 0,
                        "slide termination damage settings unavailable"
                    );
                    let extent = self.ordinary_collision_shape(id)?.positive_extent[1];
                    let height = i32::try_from(
                        support.actor_height(extent, query.scene.settings.standing_height),
                    )
                    .context("ordinary support origin outside Q8 range")?;
                    self.spatial
                        .as_mut()
                        .context("ground response spatial state unavailable")?
                        .actor_mut(id.index)?
                        .local
                        .position[1] = height;
                    ensure!(
                        !prevalidation.sliding,
                        "slope suppression and combat-mode owner unavailable"
                    );
                } else if self.actors[id.index].attributes[107] != 0 {
                    self.actors[id.index]
                        .control
                        .as_mut()
                        .expect("admitted ground control")
                        .falling_state = 0;
                } else {
                    anyhow::bail!("support miss requires falling entry and vertical timing owner");
                }
            }
            self.retain_ordinary_stable_position(id)?;
        }
        let control = self.actors[id.index]
            .control
            .as_ref()
            .expect("admitted body control");
        if mask & 1 != 0
            && (matches!(control.living_state, 0 | 15) || (27..=32).contains(&control.living_state))
        {
            // Ordinary entry clears the extra-contact append flag. These admitted
            // stages do not produce an appended normal; heightmap availability
            // is independent of that flag and still feeds the support queries.
            let contacts = self.query_ordinary_body(
                id,
                super::body::OrdinaryBodyQuery {
                    scene: query.scene,
                    animations: query.animations,
                    prepared: query.prepared,
                    sidedness: false,
                    excluded_candidate_type: None,
                    terrain: None,
                },
            )?;
            self.resolve_ordinary_primary_contacts(id, &contacts, &query.scene.settings)?;
            let contacts = self.query_ordinary_residual_body(
                id,
                super::body::OrdinaryResidualQuery {
                    scene: query.scene,
                    animations: query.animations,
                    prepared: query.prepared,
                },
            )?;
            self.resolve_ordinary_residual_contacts(id, &contacts, &query.scene.settings)?;
        }
        self.actors[id.index].attributes[6] = self.actors[id.index].attributes[115];
        let control = self.actors[id.index]
            .control
            .as_ref()
            .expect("admitted recovery control");
        let moving = control.falling_state != 0 || control.sliding_state != 0;
        let recover = if self.actors[id.index].attributes[89] != 0 && moving {
            true
        } else if rollback.landing_recovery {
            moving
                || self
                    .surface_veto
                    .context("landing recovery shared surface veto owner unavailable")?
        } else {
            false
        };
        if recover {
            self.recover_ordinary_stable(id, recovery, rollback, sounds)?;
        }
        self.rollback_ordinary_water(id, None, rollback, sounds)?;
        Ok(())
    }
}
