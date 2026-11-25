//! Fixture access excluded from production builds.
use super::*;

/// State seeding and inspection for integration fixtures that cannot use ordinary stages.
/// Available only with the `test-support` feature; not a gameplay host interface.
pub trait ScriptWorldTestExt {
    /// Test hook: set the actor that owns the pending activation prompt.
    fn set_activation_owner_for_test(&mut self, owner: Option<ScriptActorId>);

    /// Test hook: attach or release a logical actor on a rope without running the attach routine.
    fn set_rope_attached_for_test(&mut self, id: ScriptActorId, attached: bool);

    /// Test hook: set a logical actor's queued motion.
    fn set_queued_motion_for_test(&mut self, id: ScriptActorId, motion: JumpMotion);

    /// Test hook: set a logical actor's pending and accepted directional action words.
    fn set_action_words_for_test(&mut self, id: ScriptActorId, pending: u32, accepted: u32);

    /// Test hook: give a logical actor a cached link position, as an `Offset` link does.
    fn set_link_position_for_test(&mut self, id: ScriptActorId, position: [i32; 3]);

    /// Test hook: give a logical actor a default combat record.
    fn set_combat_record_for_test(&mut self, id: ScriptActorId);

    /// Test hook: set a logical actor's collision-response word.
    fn set_last_collide_for_test(&mut self, id: ScriptActorId, bits: u32);

    /// Test hook: set a logical actor's living state.
    fn set_living_state_for_test(&mut self, id: ScriptActorId, state: LivingState);

    /// Test hook: run the bounce handler's pre-support part for one actor; true when the probe
    /// ends there.
    fn bounce_handler_prefix_for_test(&mut self, id: ScriptActorId) -> Result<bool>;

    /// Test hook: put a logical actor into a falling state with its retained fall origin (Q8 Y).
    fn set_falling_for_test(&mut self, id: ScriptActorId, state: JumpMotion, origin_y: i32);

    /// Test hook: replace an actor's local angles without refreshing its pose.
    fn set_local_angles_for_test(&mut self, id: ScriptActorId, angles: [i32; 3]);

    /// Test hook: inspect the movement-animation wait policy.
    fn animation_waits_for_test(&self) -> bool;
}

impl ScriptWorldTestExt for ScriptWorld {
    fn set_activation_owner_for_test(&mut self, owner: Option<ScriptActorId>) {
        self.activation.set_owner_for_test(owner);
    }

    fn set_rope_attached_for_test(&mut self, id: ScriptActorId, attached: bool) {
        self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state")
            .rope_attached = attached;
    }

    fn set_queued_motion_for_test(&mut self, id: ScriptActorId, motion: JumpMotion) {
        self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state")
            .queued_motion = motion;
    }

    fn set_action_words_for_test(&mut self, id: ScriptActorId, pending: u32, accepted: u32) {
        self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state")
            .actions = ActionWords { pending, accepted };
    }

    fn set_link_position_for_test(&mut self, id: ScriptActorId, position: [i32; 3]) {
        self.spatial
            .as_mut()
            .expect("test spatial state")
            .actor_mut(id.index)
            .expect("test actor spatial state")
            .set_link_cache(position);
    }

    fn set_combat_record_for_test(&mut self, id: ScriptActorId) {
        self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state")
            .combat = Some(CombatControlState::default());
    }

    fn set_last_collide_for_test(&mut self, id: ScriptActorId, bits: u32) {
        self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state")
            .last_collide = crate::script::CollisionFlags::from_bits(bits);
    }

    fn set_living_state_for_test(&mut self, id: ScriptActorId, state: LivingState) {
        self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state")
            .living_state = state;
    }

    fn bounce_handler_prefix_for_test(&mut self, id: ScriptActorId) -> Result<bool> {
        self.bounce_handler_prefix(id)
    }

    fn set_falling_for_test(&mut self, id: ScriptActorId, state: JumpMotion, origin_y: i32) {
        let control = self.actors[id.index]
            .control
            .as_mut()
            .expect("test actor has control state");
        control.falling_state = state;
        control.fall_origin_y = origin_y;
    }

    fn set_local_angles_for_test(&mut self, id: ScriptActorId, angles: [i32; 3]) {
        self.spatial
            .as_mut()
            .expect("spatial state prepared")
            .actor_mut(id.index)
            .expect("actor spatial state")
            .local
            .angles = angles;
    }

    fn animation_waits_for_test(&self) -> bool {
        self.animation_waits()
    }
}
