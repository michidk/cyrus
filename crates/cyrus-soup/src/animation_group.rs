//! Animation group numbers: the index of an actor's animation group.
//!
//! Group numbers are fixed by the engine. Constant names are the installed `SOUP386.DEF`
//! animation-group equates (its second `auto` block, groups 0 to 168) without their `anim_`
//! prefix; aliases declared there with `=` share their target's number. The engine also uses
//! groups 169 to 177, which the DEF leaves unnamed (preservation `docs/engine/animation.md`).
//! Rust code names a group through these constants; [`AnimationGroup::from_index`] is limited
//! to numbers that are data, such as a script argument or an installed group record.
use serde::{Deserialize, Serialize};
use std::fmt;

/// An animation group number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AnimationGroup(u16);

impl AnimationGroup {
    /// The group a data-supplied number names.
    pub const fn from_index(index: u16) -> Self {
        Self(index)
    }

    /// The group number.
    pub const fn index(self) -> u16 {
        self.0
    }
}

impl fmt::Display for AnimationGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl AnimationGroup {
    pub const PANIC: Self = Self(0);
    pub const IDLE1: Self = Self(1);
    pub const IDLE2: Self = Self(2);
    pub const IDLE3: Self = Self(3);
    pub const IDLE_HURT: Self = Self(4);
    pub const IDLE_TORCH: Self = Self(5);
    pub const IDLE_ROPE: Self = Self(6);
    pub const IDLE_LEDGE: Self = Self(7);
    pub const FIGHT_PANIC: Self = Self(8);
    pub const MISC0: Self = Self(9);
    pub const MISC1: Self = Self(10);
    pub const MISC2: Self = Self(11);
    pub const MISC3: Self = Self(12);
    pub const MISC4: Self = Self(13);
    pub const TURN_LEFT: Self = Self(14);
    pub const TURN_RIGHT: Self = Self(15);
    pub const TURN_LEFT_TORCH: Self = Self(16);
    pub const TURN_RIGHT_TORCH: Self = Self(17);
    pub const RUN: Self = Self(18);
    /// Alias `anim_run_forward = anim_run`.
    pub const RUN_FORWARD: Self = Self::RUN;
    pub const RUN_BACKWARD: Self = Self(19);
    pub const WALK: Self = Self(20);
    /// Alias `anim_walk_forward = anim_walk`.
    pub const WALK_FORWARD: Self = Self::WALK;
    pub const WALK_FORWARD_TORCH: Self = Self(21);
    pub const WALK_BACKWARD: Self = Self(22);
    pub const WALK_BACKWARD_TORCH: Self = Self(23);
    pub const STEP_LEFT: Self = Self(24);
    pub const STEP_LEFT_TORCH: Self = Self(25);
    pub const STEP_RIGHT: Self = Self(26);
    pub const STEP_RIGHT_TORCH: Self = Self(27);
    pub const SHIMMY_LEFT_LEDGE: Self = Self(28);
    pub const SHIMMY_RIGHT_LEDGE: Self = Self(29);
    pub const RUN_TORCH: Self = Self(30);
    pub const FIGHT_RUN: Self = Self(31);
    pub const JUMP_START: Self = Self(32);
    pub const JUMP_START_TORCH: Self = Self(33);
    pub const JUMP_RUN_START: Self = Self(34);
    /// Alias `anim_jump_right_start = anim_jump_run_start`.
    pub const JUMP_RIGHT_START: Self = Self::JUMP_RUN_START;
    pub const JUMP_RIGHT_START_TORCH: Self = Self(35);
    pub const JUMP_LEFT_START: Self = Self(36);
    pub const JUMP_LEFT_START_TORCH: Self = Self(37);
    pub const JUMP_TORCH: Self = Self(38);
    pub const JUMP: Self = Self(39);
    /// Alias `anim_jump_up = anim_jump`.
    pub const JUMP_UP: Self = Self::JUMP;
    pub const JUMP_UP_ARMS: Self = Self(40);
    pub const JUMP_FORWARD_TORCH: Self = Self(41);
    pub const JUMP_FORWARD: Self = Self(42);
    pub const JUMP_FORWARD_ARMS: Self = Self(43);
    pub const JUMP_RUN: Self = Self(44);
    /// Alias `anim_jump_right = anim_jump_run`.
    pub const JUMP_RIGHT: Self = Self::JUMP_RUN;
    pub const JUMP_RIGHT_TORCH: Self = Self(45);
    pub const JUMP_LEFT: Self = Self(46);
    pub const JUMP_LEFT_TORCH: Self = Self(47);
    pub const JUMP_RUN_ARMS: Self = Self(48);
    pub const JUMP_BACKWARD: Self = Self(49);
    /// Alias `anim_jump_spin = anim_jump_backward`.
    pub const JUMP_SPIN: Self = Self::JUMP_BACKWARD;
    pub const JUMP_BACKWARD_TORCH: Self = Self(50);
    pub const JUMP_OFF_ROPE: Self = Self(51);
    pub const JUMP_OFF_ROPE_SWORD: Self = Self(52);
    pub const JUMP_DRAW_SWORD: Self = Self(53);
    pub const ATTACK_JUMP_UP: Self = Self(54);
    pub const ATTACK_JUMP_FORWARD: Self = Self(55);
    pub const FALL: Self = Self(56);
    pub const FALL_TORCH: Self = Self(57);
    pub const FALL_UP_ARMS: Self = Self(58);
    pub const FALL_RUN: Self = Self(59);
    /// Alias `anim_fall_right = anim_fall_run`.
    pub const FALL_RIGHT: Self = Self::FALL_RUN;
    pub const FALL_RIGHT_TORCH: Self = Self(60);
    pub const FALL_LEFT: Self = Self(61);
    pub const FALL_LEFT_TORCH: Self = Self(62);
    pub const FALL_BACKWARD: Self = Self(63);
    pub const FALL_BACKWARD_TORCH: Self = Self(64);
    pub const FIRST_LAND_GROUP: Self = Self(65);
    /// Alias `anim_land = first_land_group`.
    pub const LAND: Self = Self::FIRST_LAND_GROUP;
    pub const LAND_TORCH: Self = Self(66);
    pub const LAND_RUN: Self = Self(67);
    /// Alias `anim_land_right = anim_land_run`.
    pub const LAND_RIGHT: Self = Self::LAND_RUN;
    pub const LAND_RIGHT_TORCH: Self = Self(68);
    pub const LAND_LEFT: Self = Self(69);
    pub const LAND_LEFT_TORCH: Self = Self(70);
    pub const LAND_BACKWARD: Self = Self(71);
    pub const LAND_BACKWARD_TORCH: Self = Self(72);
    pub const LAND_HURT: Self = Self(73);
    pub const LAND_DEATH: Self = Self(74);
    /// Alias `last_land_group = anim_land_death`.
    pub const LAST_LAND_GROUP: Self = Self::LAND_DEATH;
    pub const SLIDE: Self = Self(75);
    /// Alias `anim_slide_down = anim_slide`.
    pub const SLIDE_DOWN: Self = Self::SLIDE;
    /// Alias `anim_slide_forward = anim_slide`.
    pub const SLIDE_FORWARD: Self = Self::SLIDE;
    pub const SLIDE_BACKWARD: Self = Self(76);
    pub const SLIDE_FORWARD_TORCH: Self = Self(77);
    pub const SLIDE_BACKWARD_TORCH: Self = Self(78);
    pub const ROPE_PANIC: Self = Self(79);
    pub const ROPE_SWING_FORWARD: Self = Self(80);
    pub const ROPE_SWING_BACK: Self = Self(81);
    pub const ROPE_UP: Self = Self(82);
    pub const ROPE_DOWN: Self = Self(83);
    pub const ROPE_GRAB: Self = Self(84);
    pub const ROPE_JUMP: Self = Self(85);
    pub const PULL_UP_LEDGE: Self = Self(86);
    pub const DROP_OFF_LEDGE: Self = Self(87);
    pub const SWING_LEDGE: Self = Self(88);
    pub const GRAB_LEDGE: Self = Self(89);
    pub const HAND_PLANT_LEDGE: Self = Self(90);
    pub const DRAW_SWORD: Self = Self(91);
    pub const FIGHT_FORW: Self = Self(92);
    /// Alias `anim_fight_forward = anim_fight_forw`.
    pub const FIGHT_FORWARD: Self = Self::FIGHT_FORW;
    pub const FIGHT_BACK: Self = Self(93);
    /// Alias `anim_fight_backward = anim_fight_back`.
    pub const FIGHT_BACKWARD: Self = Self::FIGHT_BACK;
    pub const FIGHT_CIRCLE_LEFT: Self = Self(94);
    pub const FIGHT_CIRCLE_RIGHT: Self = Self(95);
    pub const FIGHT_TURN_LEFT: Self = Self(96);
    pub const FIGHT_TURN_RIGHT: Self = Self(97);
    pub const DEFEND_LOW: Self = Self(98);
    /// Alias `anim_defend = anim_defend_low`.
    pub const DEFEND: Self = Self::DEFEND_LOW;
    pub const DEFEND_RIGHT: Self = Self(99);
    pub const DEFEND_LEFT: Self = Self(100);
    pub const DEFEND_HIGH: Self = Self(101);
    pub const ATTACK_1: Self = Self(102);
    pub const ATTACK_2: Self = Self(103);
    pub const ATTACK_3: Self = Self(104);
    pub const ATTACK_THRUST: Self = Self(105);
    /// Alias `anim_attack_thrust_start = anim_attack_thrust`.
    pub const ATTACK_THRUST_START: Self = Self::ATTACK_THRUST;
    pub const ATTACK_LUNGE: Self = Self(106);
    pub const ATTACK_1_END: Self = Self(107);
    pub const ATTACK_2_END: Self = Self(108);
    pub const FIGHT_DISARM: Self = Self(109);
    pub const ATTACK_LOW: Self = Self(110);
    pub const FIGHT_JUMP_START: Self = Self(111);
    pub const FIGHT_JUMP: Self = Self(112);
    pub const FIGHT_FALL: Self = Self(113);
    pub const FIGHT_LAND: Self = Self(114);
    pub const FIGHT_FALL_ATTACK: Self = Self(115);
    pub const FIGHT_LAND_ATTACK: Self = Self(116);
    pub const FIGHT_HURT_1: Self = Self(117);
    /// Alias `anim_hit_attack = anim_fight_hurt_1`.
    pub const HIT_ATTACK: Self = Self::FIGHT_HURT_1;
    pub const FIGHT_HURT_2: Self = Self(118);
    pub const FIGHT_HURT_FRONT: Self = Self(119);
    pub const FIGHT_HURT_BACK: Self = Self(120);
    pub const DEATH_FIGHT_STAB: Self = Self(121);
    pub const DEATH_FIGHT_HARD: Self = Self(122);
    pub const DEATH_FIGHT_STAB_END: Self = Self(123);
    pub const DEATH_FIGHT_HARD_END: Self = Self(124);
    pub const DEATH_FIGHT_FRONT: Self = Self(125);
    pub const DEATH_FIGHT_BACK: Self = Self(126);
    pub const DEATH_FIGHT_FRONT_END: Self = Self(127);
    pub const DEATH_FIGHT_BACK_END: Self = Self(128);
    pub const SHEATH_SWORD: Self = Self(129);
    pub const FIGHT_LAND_RIGHT: Self = Self(130);
    pub const FIGHT_LAND_LEFT: Self = Self(131);
    pub const EXPLORE_HURT_1: Self = Self(132);
    pub const EXPLORE_HURT_2: Self = Self(133);
    pub const DEATH_EXPLORE: Self = Self(134);
    pub const FLASK_PANIC: Self = Self(135);
    pub const DRAW_FLASK: Self = Self(136);
    pub const SHEATH_FLASK: Self = Self(137);
    pub const CATCH_SPELL: Self = Self(138);
    pub const CHARGE_FLASK: Self = Self(139);
    pub const SHOOT_FLASK: Self = Self(140);
    pub const SWIM_IDLE: Self = Self(141);
    /// Alias `first_swim_group = anim_swim_idle`.
    pub const FIRST_SWIM_GROUP: Self = Self::SWIM_IDLE;
    pub const SWIM_FORWARD: Self = Self(142);
    pub const SWIM_BACKWARD: Self = Self(143);
    /// Alias `last_swim_group = anim_swim_backward`.
    pub const LAST_SWIM_GROUP: Self = Self::SWIM_BACKWARD;
    pub const PICKUP: Self = Self(144);
    pub const PULL_LEVER_DOWN: Self = Self(145);
    pub const PULL_LEVER_UP: Self = Self(146);
    pub const TURN_LEVER_LEFT: Self = Self(147);
    pub const TURN_LEVER_RIGHT: Self = Self(148);
    pub const LIGHT_TORCH: Self = Self(149);
    pub const UNLIGHT_TORCH: Self = Self(150);
    pub const LIGHT_WITH_TORCH: Self = Self(151);
    pub const STILL: Self = Self(152);
    pub const TALK1: Self = Self(153);
    pub const TALK2: Self = Self(154);
    pub const TALK3: Self = Self(155);
    pub const MISC5: Self = Self(156);
    pub const MISC6: Self = Self(157);
    pub const MISC7: Self = Self(158);
    pub const MISC8: Self = Self(159);
    pub const MISC9: Self = Self(160);
    pub const MISC10: Self = Self(161);
    pub const MISC11: Self = Self(162);
    pub const MISC12: Self = Self(163);
    pub const MISC13: Self = Self(164);
    pub const MISC14: Self = Self(165);
    pub const MISC15: Self = Self(166);
    pub const MISC16: Self = Self(167);
    pub const MISC17: Self = Self(168);
}

/// A group as a script value, such as `GetAnimation`'s result.
impl From<AnimationGroup> for i32 {
    fn from(group: AnimationGroup) -> Self {
        group.0.into()
    }
}

#[cfg(test)]
mod tests {
    use super::AnimationGroup;

    #[test]
    fn constants_follow_the_installed_numbering_and_aliases_share_it() {
        assert_eq!(AnimationGroup::PANIC.index(), 0);
        assert_eq!(AnimationGroup::RUN.index(), 18);
        assert_eq!(AnimationGroup::LAND.index(), 65);
        assert_eq!(AnimationGroup::LIGHT_TORCH.index(), 149);
        assert_eq!(AnimationGroup::MISC17.index(), 168);
        assert_eq!(AnimationGroup::RUN_FORWARD, AnimationGroup::RUN);
        assert_eq!(
            AnimationGroup::LAST_SWIM_GROUP,
            AnimationGroup::SWIM_BACKWARD
        );
        assert_eq!(AnimationGroup::from_index(93), AnimationGroup::FIGHT_BACK);
    }

    #[test]
    fn groups_serialize_as_their_number() {
        let json = serde_json::to_string(&AnimationGroup::FIGHT_FORW).unwrap();
        assert_eq!(json, "92");
        let group: AnimationGroup = serde_json::from_str(&json).unwrap();
        assert_eq!(group, AnimationGroup::FIGHT_FORW);
    }
}
