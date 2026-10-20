//! The pickup and plain player-alignment movers, as decision logic over a port.
//!
//! The decision structure follows the original movers (`PlayerPickupLineUp`'s mover and the
//! plain lineup mover it falls back to) and is checked against them in an emulator with their
//! leaf helpers logged. The port supplies the helpers: facing, arrival, movement permission, the
//! four directional steps, turning, roll leveling, the default-animation request and the
//! collision-response state.

use super::alignment::{CloseStep, close_step, keeps_step, relative_bearing};

/// The leaf helpers the movers call, each answering as the original helper does.
pub trait LineupPort {
    /// A collision response left unresolved for over eight updates.
    fn stale_response(&self) -> bool;
    /// Level the player's roll.
    fn level_roll(&mut self);
    /// Whether the straight path from the player to `target` is free.
    fn path_clear(&mut self, target: [i32; 3]) -> bool;
    /// Face the point; true while still turning.
    fn face_point(&mut self, x: i32, z: i32) -> bool;
    /// The facing arrival test: true after snapping onto the point.
    fn arrival(&mut self, x: i32, z: i32) -> bool;
    /// The movement-distance check.
    fn permitted(&mut self, x: i32, z: i32) -> bool;
    /// One directional step toward the point; true when it was kept.
    fn step(&mut self, step: CloseStep, x: i32, z: i32) -> bool;
    /// Turn toward a heading; true while still turning.
    fn turn_to(&mut self, heading: u16) -> bool;
    /// The player's heading.
    fn heading(&self) -> u16;
    /// The player's heading minus the heading of the displacement to the point, wrapped.
    fn relative_bearing(&self, x: i32, z: i32) -> u16;
    /// Request the default animation; the helper reports completion (always true in the original).
    fn default_animation(&mut self) -> bool;
    /// Set or clear the four directions' shared called flags.
    fn set_called_flags(&mut self, value: bool);
    /// Report a stale collision response: return state 6.
    fn abandon(&mut self, clear_hook_flag: bool);
    /// The packed collision-response word carries a mark that makes alignment fall back.
    fn response_masked(&self) -> bool;
    /// The collision-response age is above eight updates.
    fn response_age_over(&self) -> bool;
    /// Set the player's alignment flag (script-control bit 2).
    fn set_hook_flag(&mut self);
    /// Clear the player's alignment flag.
    fn clear_hook_flag(&mut self);
}

/// The player state a directional step helper moves and, when it rejects the step, restores.
pub trait StepPort {
    /// Position, step counter and animation state captured before the step.
    type Saved;
    fn position(&self) -> [i32; 3];
    fn heading(&self) -> u16;
    fn save(&mut self) -> Self::Saved;
    fn restore(&mut self, saved: Self::Saved);
    /// One step of the matching one-unit movement task.
    fn walk(&mut self, step: CloseStep);
    /// Clear the step's called flag.
    fn clear_called(&mut self, step: CloseStep);
    fn set_horizontal(&mut self, x: i32, z: i32);
}

/// A directional close step: walk once, test the result against the target and, when the test
/// fails, restore the saved state and snap onto the target. True when the step was kept.
pub fn close_step_run<P: StepPort>(port: &mut P, step: CloseStep, x: i32, z: i32) -> bool {
    let distance = |port: &P| {
        let [px, _, pz] = port.position();
        // The sum of squares is the original's single-precision length; coordinates fit i32.
        cyrus_collision::distance::rounded_distance([px - x, 0, pz - z]).unwrap_or(i32::MAX)
    };
    let before = distance(port);
    let saved = port.save();
    port.walk(step);
    let [px, _, pz] = port.position();
    let after = relative_bearing([x - px, z - pz], port.heading());
    if keeps_step(step, after, (before, distance(port))) {
        return true;
    }
    port.clear_called(step);
    port.restore(saved);
    port.set_horizontal(x, z);
    false
}

/// The owner of the selected player's alignment: the helpers plus the reference actor and any
/// failure a helper met (the helpers answer conservatively and the call then fails).
pub trait LineupHost: LineupPort {
    /// The reference actor and its pose: the retained actor index when one is given, otherwise the
    /// receiving actor.
    fn reference(&mut self, retained: Option<u32>) -> anyhow::Result<(u32, Reference)>;
    /// The rounded three-axis distance between the player and the receiving actor.
    fn receiver_distance(&mut self) -> anyhow::Result<i32>;
    /// The installed lineup distance in Q8 units.
    fn lineup_distance(&mut self) -> anyhow::Result<i32>;
    /// Store `PlayerLineUp`'s return value as the shared player-lineup result.
    fn set_lineup_result(&mut self, result: i32);
    /// The first failure a helper met, if any.
    fn failure(&mut self) -> Option<anyhow::Error>;
}

/// The reference actor `PlayerLineUp` aligns to: its position and heading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reference {
    pub position: [i32; 3],
    pub heading: u16,
}

/// The target and final heading of an alignment: the reference heading plus `angle`, the offsets
/// rotated by it and added to the reference's X/Z.
fn rotated_target(
    angle: i32,
    offsets: [i32; 2],
    reference: Reference,
) -> anyhow::Result<(u16, [i32; 2])> {
    let heading = ((i32::from(reference.heading) + angle) & 0x7ff) as u16;
    let [x, z] = cyrus_world::transform::rotate_offset(offsets, heading)?;
    Ok((
        heading,
        [
            x.wrapping_add(reference.position[0]),
            z.wrapping_add(reference.position[2]),
        ],
    ))
}

/// Complete the turn to the final heading: still turning keeps the alignment going; finished, the
/// default animation request ends it, clearing the alignment flag.
fn finish_turn<P: LineupPort + ?Sized>(port: &mut P, heading: u16) -> i32 {
    if port.turn_to((heading + 0x400) & 0x7ff) {
        return 1;
    }
    if port.default_animation() {
        port.clear_hook_flag();
        0
    } else {
        1
    }
}

/// The retained fallback flag of `PlayerLineUp`'s movers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AlignState {
    pub fallback: bool,
}

/// One directional step toward the point when movement is permitted; a failed step sets the
/// fallback.
fn align_step<P: LineupPort + ?Sized>(
    port: &mut P,
    state: &mut AlignState,
    step: CloseStep,
    x: i32,
    z: i32,
) {
    if port.permitted(x, z) && !port.step(step, x, z) {
        state.fallback = true;
    }
}

/// `PlayerLineUp`'s approach mover (the player is far from the receiver): face the destination
/// and walk forward, snapping and turning to the final heading on arrival. The fallback faces the
/// reference's own position instead. Zero when done, one while continuing.
pub fn approach_mover<P: LineupPort + ?Sized>(
    port: &mut P,
    state: &mut AlignState,
    angle: i32,
    offsets: [i32; 2],
    reference: Reference,
) -> anyhow::Result<i32> {
    if state.fallback || port.response_masked() {
        if port.face_point(reference.position[0], reference.position[2]) {
            return Ok(1);
        }
        port.abandon(true);
        return Ok(0);
    }
    port.level_roll();
    port.set_hook_flag();
    let (heading, [x, z]) = rotated_target(angle, offsets, reference)?;
    if port.arrival(x, z) {
        return Ok(finish_turn(port, heading));
    }
    if !port.face_point(x, z) {
        align_step(port, state, CloseStep::Forward, x, z);
        if port.response_masked() && port.response_age_over() {
            state.fallback = true;
        }
    }
    Ok(1)
}

/// `PlayerLineUp`'s close mover: turn to the final heading, then step in the direction the
/// bearing to the destination selects. The fallback only turns to the final heading.
pub fn close_mover<P: LineupPort + ?Sized>(
    port: &mut P,
    state: &mut AlignState,
    angle: i32,
    offsets: [i32; 2],
    reference: Reference,
) -> anyhow::Result<i32> {
    let final_heading = ((i32::from(reference.heading) + angle) & 0x7ff) as u16;
    if state.fallback {
        if port.turn_to((final_heading + 0x400) & 0x7ff) {
            return Ok(1);
        }
        if !port.default_animation() {
            return Ok(1);
        }
        port.abandon(true);
        return Ok(0);
    }
    port.level_roll();
    port.set_hook_flag();
    let (heading, [x, z]) = rotated_target(angle, offsets, reference)?;
    if port.arrival(x, z) {
        return Ok(finish_turn(port, heading));
    }
    if port.turn_to((heading + 0x400) & 0x7ff) {
        return Ok(1);
    }
    let bearing = port.relative_bearing(x, z);
    align_step(port, state, close_step(bearing), x, z);
    if port.response_masked() {
        state.fallback = true;
    }
    Ok(1)
}

/// The pickup mover's retained state: the fallback flag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PickupState {
    pub fallback: bool,
}

/// Completion: the default animation request, which reports completion.
fn finished<P: LineupPort + ?Sized>(port: &mut P) -> i32 {
    i32::from(!port.default_animation())
}

fn stale_exit<P: LineupPort + ?Sized>(port: &mut P) -> i32 {
    if port.stale_response() {
        port.abandon(true);
        0
    } else {
        1
    }
}

/// `PlayerPickupLineUp`'s mover: zero when done, one while continuing. On the first invocation
/// (elapsed zero) a blocked straight path selects the fallback, the plain mover, for good.
pub fn pickup_mover<P: LineupPort + ?Sized>(
    port: &mut P,
    state: &mut PickupState,
    elapsed: i32,
    angle: i32,
    target: [i32; 3],
) -> i32 {
    if elapsed == 0 && !state.fallback && !port.path_clear(target) {
        state.fallback = true;
    }
    if state.fallback {
        return plain_mover(port, angle, target);
    }
    let [x, _, z] = target;
    if port.stale_response() {
        port.abandon(false);
        return 0;
    }
    port.level_roll();
    if port.face_point(x, z) {
        return 1;
    }
    if port.arrival(x, z) {
        port.set_called_flags(false);
        return finished(port);
    }
    if port.permitted(x, z) && !port.step(CloseStep::Forward, x, z) {
        port.set_called_flags(false);
        return finished(port);
    }
    stale_exit(port)
}

/// The plain mover: far from the final heading it backs toward the target while turning to face
/// it, otherwise it turns to the final heading and steps in the direction the bearing selects.
pub fn plain_mover<P: LineupPort + ?Sized>(port: &mut P, angle: i32, target: [i32; 3]) -> i32 {
    let [x, _, z] = target;
    if port.stale_response() {
        port.abandon(false);
        return 0;
    }
    port.level_roll();
    let heading = (angle & 0x7ff) as u16;
    let final_heading = (heading + 0x400) & 0x7ff;
    let difference = {
        let delta =
            (i32::from(port.heading()) - i32::from(final_heading) + 1024).rem_euclid(2048) - 1024;
        delta.abs()
    };
    if difference > 0x300 {
        port.turn_to((final_heading + 0x400) & 0x7ff);
        if port.permitted(x, z) && !port.step(CloseStep::Backward, x, z) {
            if !port.turn_to(final_heading) {
                port.set_called_flags(true);
                return finished(port);
            }
            return 1;
        }
        return stale_exit(port);
    }
    if port.turn_to(final_heading) {
        return 1;
    }
    if port.arrival(x, z) {
        port.set_called_flags(true);
        return finished(port);
    }
    let bearing = port.relative_bearing(x, z);
    if port.permitted(x, z) {
        port.step(close_step(bearing), x, z);
    }
    stale_exit(port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A scripted port: each helper answers from its script in call order and logs its call.
    struct Scripted {
        stale: bool,
        heading: u16,
        position: [i32; 2],
        script: BTreeMap<String, Vec<i32>>,
        used: BTreeMap<String, usize>,
        log: Vec<(String, Vec<i32>)>,
        called: i32,
        state: i32,
        hook: bool,
        masked: bool,
        age_over: bool,
    }

    impl Scripted {
        fn answer(&mut self, name: &str, args: Vec<i32>) -> bool {
            let index = self.used.entry(name.into()).or_default();
            let value = self.script[name][*index % self.script[name].len()] != 0;
            *index += 1;
            self.log.push((name.into(), args));
            value
        }
    }

    impl LineupPort for Scripted {
        fn stale_response(&self) -> bool {
            self.stale
        }
        fn level_roll(&mut self) {}
        fn path_clear(&mut self, target: [i32; 3]) -> bool {
            self.answer("path", target.to_vec())
        }
        fn face_point(&mut self, x: i32, z: i32) -> bool {
            self.answer("face", vec![x, z])
        }
        fn arrival(&mut self, x: i32, z: i32) -> bool {
            self.answer("arrive", vec![x, z])
        }
        fn permitted(&mut self, x: i32, z: i32) -> bool {
            self.answer("permit", vec![x, z])
        }
        fn step(&mut self, step: CloseStep, x: i32, z: i32) -> bool {
            let name = match step {
                CloseStep::Forward => "fwd",
                CloseStep::Right => "right",
                CloseStep::Backward => "back",
                CloseStep::Left => "left",
            };
            self.answer(name, vec![x, z])
        }
        fn turn_to(&mut self, heading: u16) -> bool {
            self.answer("turn", vec![i32::from(heading)])
        }
        fn heading(&self) -> u16 {
            self.heading
        }
        fn relative_bearing(&self, x: i32, z: i32) -> u16 {
            crate::script::alignment::relative_bearing(
                [x - self.position[0], z - self.position[1]],
                self.heading,
            )
        }
        fn default_animation(&mut self) -> bool {
            self.answer("anim", vec![])
        }
        fn set_called_flags(&mut self, value: bool) {
            self.called = i32::from(value);
        }
        fn abandon(&mut self, clear_hook_flag: bool) {
            self.state = 6;
            if clear_hook_flag {
                self.hook = false;
            }
        }
        fn response_masked(&self) -> bool {
            self.masked
        }
        fn response_age_over(&self) -> bool {
            self.age_over
        }
        fn set_hook_flag(&mut self) {
            self.hook = true;
        }
        fn clear_hook_flag(&mut self) {
            self.hook = false;
        }
    }

    #[test]
    fn the_movers_match_the_original_routines_with_logged_helpers() {
        // Rows from the original movers in an emulator with their leaf helpers stubbed to scripted
        // answers: mover (0 pickup, 1 plain), scenario, scripted answers, then the helper calls
        // the original made, its return, return state, hook flag, called flags and fallback flag.
        #[derive(serde::Deserialize)]
        struct Row {
            m: u8,
            stale: u8,
            heading: u16,
            pos: [i32; 3],
            target: [i32; 3],
            angle: i32,
            elapsed: i32,
            fallback: u8,
            script: BTreeMap<String, Vec<i32>>,
            log: Vec<Vec<serde_json::Value>>,
            ret: i32,
            state: i32,
            flag: i32,
            called: [i32; 4],
            fb: u8,
        }
        let rows: Vec<Row> = serde_json::from_str(
            r#"[
{"m":0,"stale":0,"heading":100,"pos":[5000,0,0],"target":[3000,0,0],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[0,0,0],"face":[0,0,1],"arrive":[1,1,1],"permit":[1,0,1],"fwd":[0,1,1],"right":[0,1,1],"back":[0,1,1],"left":[1,0,1],"anim":[1,1,1],"turn":[0,0,1]},"log":[["path",3000,0,0],["turn",0],["arrive",3000,0],["anim"]],"ret":0,"state":0,"flag":3,"called":[1,1,1,1],"fb":1},
{"m":1,"stale":0,"heading":100,"pos":[5000,0,0],"target":[3000,0,0],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[0,0,0],"face":[0,0,1],"arrive":[1,1,1],"permit":[1,0,1],"fwd":[0,1,1],"right":[0,1,1],"back":[0,1,1],"left":[1,0,1],"anim":[1,1,1],"turn":[0,0,1]},"log":[["turn",0],["arrive",3000,0],["anim"]],"ret":0,"state":0,"flag":3,"called":[1,1,1,1],"fb":0},
{"m":0,"stale":0,"heading":0,"pos":[0,3000,256],"target":[-512,0,256],"angle":1900,"elapsed":5,"fallback":1,"script":{"path":[1,0,1],"face":[0,0,1],"arrive":[0,0,1],"permit":[1,1,1],"fwd":[1,1,0],"right":[1,0,1],"back":[0,1,0],"left":[0,0,1],"anim":[0,1,1],"turn":[1,1,1]},"log":[["turn",1900],["permit",-512,256],["back",-512,256],["turn",876]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":0,"heading":0,"pos":[0,3000,256],"target":[-512,0,256],"angle":1900,"elapsed":5,"fallback":1,"script":{"path":[1,0,1],"face":[0,0,1],"arrive":[0,0,1],"permit":[1,1,1],"fwd":[1,1,0],"right":[1,0,1],"back":[0,1,0],"left":[0,0,1],"anim":[0,1,1],"turn":[1,1,1]},"log":[["turn",1900],["permit",-512,256],["back",-512,256],["turn",876]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":0,"stale":0,"heading":1500,"pos":[256,3000,-2560],"target":[0,0,256],"angle":1900,"elapsed":1,"fallback":0,"script":{"path":[1,0,1],"face":[1,1,1],"arrive":[1,1,0],"permit":[1,0,0],"fwd":[0,0,1],"right":[1,1,0],"back":[1,0,1],"left":[1,1,0],"anim":[1,1,1],"turn":[1,1,1]},"log":[["face",0,256]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":1500,"pos":[256,3000,-2560],"target":[0,0,256],"angle":1900,"elapsed":1,"fallback":0,"script":{"path":[1,0,1],"face":[1,1,1],"arrive":[1,1,0],"permit":[1,0,0],"fwd":[0,0,1],"right":[1,1,0],"back":[1,0,1],"left":[1,1,0],"anim":[1,1,1],"turn":[1,1,1]},"log":[["turn",876]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":1500,"pos":[0,3000,0],"target":[-512,0,400],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[0,1,1],"face":[0,0,1],"arrive":[0,0,1],"permit":[1,1,1],"fwd":[1,0,0],"right":[0,1,0],"back":[0,1,1],"left":[1,0,1],"anim":[0,1,1],"turn":[1,1,1]},"log":[["path",-512,0,400]],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":1,"heading":1500,"pos":[0,3000,0],"target":[-512,0,400],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[0,1,1],"face":[0,0,1],"arrive":[0,0,1],"permit":[1,1,1],"fwd":[1,0,0],"right":[0,1,0],"back":[0,1,1],"left":[1,0,1],"anim":[0,1,1],"turn":[1,1,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":2047,"pos":[256,3000,-2560],"target":[256,0,400],"angle":300,"elapsed":0,"fallback":1,"script":{"path":[1,1,0],"face":[1,1,0],"arrive":[1,1,0],"permit":[1,1,1],"fwd":[0,1,0],"right":[1,0,1],"back":[0,1,1],"left":[0,1,1],"anim":[1,0,1],"turn":[1,0,0]},"log":[["turn",1324]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":0,"heading":2047,"pos":[256,3000,-2560],"target":[256,0,400],"angle":300,"elapsed":0,"fallback":1,"script":{"path":[1,1,0],"face":[1,1,0],"arrive":[1,1,0],"permit":[1,1,1],"fwd":[0,1,0],"right":[1,0,1],"back":[0,1,1],"left":[0,1,1],"anim":[1,0,1],"turn":[1,0,0]},"log":[["turn",1324]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":0,"stale":0,"heading":100,"pos":[-512,3000,9000],"target":[3000,0,256],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[0,1,0],"face":[1,1,0],"arrive":[1,1,0],"permit":[0,1,0],"fwd":[1,0,0],"right":[1,1,1],"back":[0,1,1],"left":[1,1,1],"anim":[1,1,1],"turn":[0,0,0]},"log":[["path",3000,0,256],["turn",0],["arrive",3000,256],["anim"]],"ret":0,"state":0,"flag":3,"called":[1,1,1,1],"fb":1},
{"m":1,"stale":0,"heading":100,"pos":[-512,3000,9000],"target":[3000,0,256],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[0,1,0],"face":[1,1,0],"arrive":[1,1,0],"permit":[0,1,0],"fwd":[1,0,0],"right":[1,1,1],"back":[0,1,1],"left":[1,1,1],"anim":[1,1,1],"turn":[0,0,0]},"log":[["turn",0],["arrive",3000,256],["anim"]],"ret":0,"state":0,"flag":3,"called":[1,1,1,1],"fb":0},
{"m":0,"stale":0,"heading":1500,"pos":[5000,3000,0],"target":[-512,0,0],"angle":300,"elapsed":5,"fallback":0,"script":{"path":[1,0,1],"face":[0,1,1],"arrive":[0,0,1],"permit":[0,1,0],"fwd":[1,1,1],"right":[1,1,0],"back":[1,0,0],"left":[1,0,1],"anim":[0,1,1],"turn":[1,1,1]},"log":[["face",-512,0],["arrive",-512,0],["permit",-512,0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":1500,"pos":[5000,3000,0],"target":[-512,0,0],"angle":300,"elapsed":5,"fallback":0,"script":{"path":[1,0,1],"face":[0,1,1],"arrive":[0,0,1],"permit":[0,1,0],"fwd":[1,1,1],"right":[1,1,0],"back":[1,0,0],"left":[1,0,1],"anim":[0,1,1],"turn":[1,1,1]},"log":[["turn",1324]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":700,"pos":[0,0,0],"target":[0,0,-2560],"angle":1900,"elapsed":0,"fallback":1,"script":{"path":[1,1,1],"face":[0,1,1],"arrive":[1,1,1],"permit":[0,1,0],"fwd":[0,1,1],"right":[0,1,0],"back":[0,1,1],"left":[1,1,1],"anim":[1,0,1],"turn":[1,1,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":1,"heading":700,"pos":[0,0,0],"target":[0,0,-2560],"angle":1900,"elapsed":0,"fallback":1,"script":{"path":[1,1,1],"face":[0,1,1],"arrive":[1,1,1],"permit":[0,1,0],"fwd":[0,1,1],"right":[0,1,0],"back":[0,1,1],"left":[1,1,1],"anim":[1,0,1],"turn":[1,1,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":0,"stale":0,"heading":2047,"pos":[256,0,256],"target":[0,0,0],"angle":0,"elapsed":0,"fallback":0,"script":{"path":[0,1,1],"face":[1,0,1],"arrive":[1,0,1],"permit":[1,1,1],"fwd":[1,0,0],"right":[1,1,0],"back":[0,1,1],"left":[1,0,1],"anim":[1,1,1],"turn":[1,0,1]},"log":[["path",0,0,0],["turn",0],["permit",0,0],["back",0,0],["turn",1024],["anim"]],"ret":0,"state":0,"flag":3,"called":[1,1,1,1],"fb":1},
{"m":1,"stale":0,"heading":2047,"pos":[256,0,256],"target":[0,0,0],"angle":0,"elapsed":0,"fallback":0,"script":{"path":[0,1,1],"face":[1,0,1],"arrive":[1,0,1],"permit":[1,1,1],"fwd":[1,0,0],"right":[1,1,0],"back":[0,1,1],"left":[1,0,1],"anim":[1,1,1],"turn":[1,0,1]},"log":[["turn",0],["permit",0,0],["back",0,0],["turn",1024],["anim"]],"ret":0,"state":0,"flag":3,"called":[1,1,1,1],"fb":0},
{"m":0,"stale":0,"heading":1024,"pos":[5000,3000,0],"target":[3000,0,256],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[0,1,1],"face":[0,0,1],"arrive":[0,1,1],"permit":[1,1,0],"fwd":[0,0,0],"right":[1,1,1],"back":[1,1,0],"left":[1,1,0],"anim":[1,1,1],"turn":[1,1,0]},"log":[["path",3000,0,256],["turn",1024],["permit",3000,256],["back",3000,256]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":0,"heading":1024,"pos":[5000,3000,0],"target":[3000,0,256],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[0,1,1],"face":[0,0,1],"arrive":[0,1,1],"permit":[1,1,0],"fwd":[0,0,0],"right":[1,1,1],"back":[1,1,0],"left":[1,1,0],"anim":[1,1,1],"turn":[1,1,0]},"log":[["turn",1024],["permit",3000,256],["back",3000,256]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":100,"pos":[256,3000,9000],"target":[3000,0,-2560],"angle":300,"elapsed":1,"fallback":0,"script":{"path":[1,0,1],"face":[1,1,0],"arrive":[0,1,0],"permit":[1,1,1],"fwd":[1,1,1],"right":[1,1,1],"back":[1,0,0],"left":[1,0,1],"anim":[0,1,0],"turn":[1,1,0]},"log":[["face",3000,-2560]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":100,"pos":[256,3000,9000],"target":[3000,0,-2560],"angle":300,"elapsed":1,"fallback":0,"script":{"path":[1,0,1],"face":[1,1,0],"arrive":[0,1,0],"permit":[1,1,1],"fwd":[1,1,1],"right":[1,1,1],"back":[1,0,0],"left":[1,0,1],"anim":[0,1,0],"turn":[1,1,0]},"log":[["turn",300],["permit",3000,-2560],["back",3000,-2560]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":2047,"pos":[256,3000,-2560],"target":[-512,0,0],"angle":1900,"elapsed":1,"fallback":0,"script":{"path":[1,0,1],"face":[1,1,1],"arrive":[0,1,1],"permit":[0,0,1],"fwd":[0,0,0],"right":[1,0,1],"back":[0,1,0],"left":[1,0,1],"anim":[1,1,1],"turn":[0,0,0]},"log":[["face",-512,0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":2047,"pos":[256,3000,-2560],"target":[-512,0,0],"angle":1900,"elapsed":1,"fallback":0,"script":{"path":[1,0,1],"face":[1,1,1],"arrive":[0,1,1],"permit":[0,0,1],"fwd":[0,0,0],"right":[1,0,1],"back":[0,1,0],"left":[1,0,1],"anim":[1,1,1],"turn":[0,0,0]},"log":[["turn",1900],["permit",-512,0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":700,"pos":[5000,3000,-2560],"target":[256,0,9000],"angle":300,"elapsed":0,"fallback":1,"script":{"path":[1,1,1],"face":[1,0,1],"arrive":[1,0,1],"permit":[1,1,1],"fwd":[1,1,0],"right":[1,0,0],"back":[1,0,1],"left":[0,1,1],"anim":[0,1,0],"turn":[0,1,0]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":1,"heading":700,"pos":[5000,3000,-2560],"target":[256,0,9000],"angle":300,"elapsed":0,"fallback":1,"script":{"path":[1,1,1],"face":[1,0,1],"arrive":[1,0,1],"permit":[1,1,1],"fwd":[1,1,0],"right":[1,0,0],"back":[1,0,1],"left":[0,1,1],"anim":[0,1,0],"turn":[0,1,0]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":0,"stale":0,"heading":100,"pos":[-512,0,256],"target":[3000,0,9000],"angle":0,"elapsed":1,"fallback":0,"script":{"path":[1,1,1],"face":[0,1,0],"arrive":[1,0,0],"permit":[1,1,1],"fwd":[1,1,0],"right":[1,1,0],"back":[0,0,1],"left":[1,1,1],"anim":[0,1,1],"turn":[1,1,0]},"log":[["face",3000,9000],["arrive",3000,9000],["anim"]],"ret":1,"state":0,"flag":3,"called":[0,0,0,0],"fb":0},
{"m":1,"stale":0,"heading":100,"pos":[-512,0,256],"target":[3000,0,9000],"angle":0,"elapsed":1,"fallback":0,"script":{"path":[1,1,1],"face":[0,1,0],"arrive":[1,0,0],"permit":[1,1,1],"fwd":[1,1,0],"right":[1,1,0],"back":[0,0,1],"left":[1,1,1],"anim":[0,1,1],"turn":[1,1,0]},"log":[["turn",0],["permit",3000,9000],["back",3000,9000],["turn",1024]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":1024,"pos":[-512,3000,9000],"target":[256,0,9000],"angle":300,"elapsed":0,"fallback":0,"script":{"path":[1,1,1],"face":[1,0,1],"arrive":[0,1,0],"permit":[1,0,1],"fwd":[1,0,0],"right":[1,1,0],"back":[1,1,1],"left":[1,0,1],"anim":[1,1,1],"turn":[1,0,0]},"log":[["path",256,0,9000],["face",256,9000]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":1024,"pos":[-512,3000,9000],"target":[256,0,9000],"angle":300,"elapsed":0,"fallback":0,"script":{"path":[1,1,1],"face":[1,0,1],"arrive":[0,1,0],"permit":[1,0,1],"fwd":[1,0,0],"right":[1,1,0],"back":[1,1,1],"left":[1,0,1],"anim":[1,1,1],"turn":[1,0,0]},"log":[["turn",1324]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":100,"pos":[256,0,9000],"target":[0,0,0],"angle":0,"elapsed":5,"fallback":0,"script":{"path":[0,1,1],"face":[0,1,1],"arrive":[1,1,0],"permit":[1,1,1],"fwd":[1,0,0],"right":[1,1,1],"back":[1,1,1],"left":[0,0,0],"anim":[0,1,0],"turn":[0,1,1]},"log":[["face",0,0],["arrive",0,0],["anim"]],"ret":1,"state":0,"flag":3,"called":[0,0,0,0],"fb":0},
{"m":1,"stale":0,"heading":100,"pos":[256,0,9000],"target":[0,0,0],"angle":0,"elapsed":5,"fallback":0,"script":{"path":[0,1,1],"face":[0,1,1],"arrive":[1,1,0],"permit":[1,1,1],"fwd":[1,0,0],"right":[1,1,1],"back":[1,1,1],"left":[0,0,0],"anim":[0,1,0],"turn":[0,1,1]},"log":[["turn",0],["permit",0,0],["back",0,0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":0,"pos":[-512,3000,9000],"target":[5000,0,0],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[1,0,0],"face":[0,0,0],"arrive":[1,1,0],"permit":[1,1,0],"fwd":[0,1,0],"right":[1,0,1],"back":[1,0,1],"left":[1,1,1],"anim":[1,1,1],"turn":[1,0,0]},"log":[["path",5000,0,0],["face",5000,0],["arrive",5000,0],["anim"]],"ret":0,"state":0,"flag":3,"called":[0,0,0,0],"fb":0},
{"m":1,"stale":0,"heading":0,"pos":[-512,3000,9000],"target":[5000,0,0],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[1,0,0],"face":[0,0,0],"arrive":[1,1,0],"permit":[1,1,0],"fwd":[0,1,0],"right":[1,0,1],"back":[1,0,1],"left":[1,1,1],"anim":[1,1,1],"turn":[1,0,0]},"log":[["turn",0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":100,"pos":[0,0,-2560],"target":[0,0,9000],"angle":1900,"elapsed":1,"fallback":0,"script":{"path":[1,1,0],"face":[0,1,1],"arrive":[1,1,0],"permit":[0,0,1],"fwd":[1,1,1],"right":[1,0,0],"back":[1,0,0],"left":[0,0,0],"anim":[0,0,0],"turn":[1,1,0]},"log":[["face",0,9000],["arrive",0,9000],["anim"]],"ret":1,"state":0,"flag":3,"called":[0,0,0,0],"fb":0},
{"m":1,"stale":0,"heading":100,"pos":[0,0,-2560],"target":[0,0,9000],"angle":1900,"elapsed":1,"fallback":0,"script":{"path":[1,1,0],"face":[0,1,1],"arrive":[1,1,0],"permit":[0,0,1],"fwd":[1,1,1],"right":[1,0,0],"back":[1,0,0],"left":[0,0,0],"anim":[0,0,0],"turn":[1,1,0]},"log":[["turn",1900],["permit",0,9000]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":1500,"pos":[5000,0,-2560],"target":[3000,0,400],"angle":300,"elapsed":1,"fallback":0,"script":{"path":[0,0,1],"face":[1,0,1],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,1,0],"right":[1,0,0],"back":[1,1,1],"left":[1,1,1],"anim":[0,0,1],"turn":[1,1,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":1,"heading":1500,"pos":[5000,0,-2560],"target":[3000,0,400],"angle":300,"elapsed":1,"fallback":0,"script":{"path":[0,0,1],"face":[1,0,1],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,1,0],"right":[1,0,0],"back":[1,1,1],"left":[1,1,1],"anim":[0,0,1],"turn":[1,1,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":700,"pos":[-512,0,-2560],"target":[3000,0,256],"angle":1900,"elapsed":1,"fallback":0,"script":{"path":[0,1,1],"face":[0,1,0],"arrive":[1,0,0],"permit":[1,1,1],"fwd":[1,1,0],"right":[0,1,0],"back":[0,0,0],"left":[1,0,0],"anim":[1,1,1],"turn":[1,0,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":1,"heading":700,"pos":[-512,0,-2560],"target":[3000,0,256],"angle":1900,"elapsed":1,"fallback":0,"script":{"path":[0,1,1],"face":[0,1,0],"arrive":[1,0,0],"permit":[1,1,1],"fwd":[1,1,0],"right":[0,1,0],"back":[0,0,0],"left":[1,0,0],"anim":[1,1,1],"turn":[1,0,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":1024,"pos":[5000,0,9000],"target":[256,0,256],"angle":0,"elapsed":1,"fallback":0,"script":{"path":[0,0,1],"face":[1,1,1],"arrive":[0,0,1],"permit":[0,1,0],"fwd":[0,1,1],"right":[1,1,1],"back":[1,1,0],"left":[0,1,1],"anim":[1,0,1],"turn":[0,0,0]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":1,"heading":1024,"pos":[5000,0,9000],"target":[256,0,256],"angle":0,"elapsed":1,"fallback":0,"script":{"path":[0,0,1],"face":[1,1,1],"arrive":[0,0,1],"permit":[0,1,0],"fwd":[0,1,1],"right":[1,1,1],"back":[1,1,0],"left":[0,1,1],"anim":[1,0,1],"turn":[0,0,0]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":2047,"pos":[0,0,0],"target":[0,0,400],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[1,1,0],"face":[1,0,1],"arrive":[0,0,0],"permit":[1,1,1],"fwd":[1,1,0],"right":[1,1,1],"back":[0,1,0],"left":[1,1,1],"anim":[0,1,0],"turn":[1,0,1]},"log":[["path",0,0,400],["face",0,400]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":2047,"pos":[0,0,0],"target":[0,0,400],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[1,1,0],"face":[1,0,1],"arrive":[0,0,0],"permit":[1,1,1],"fwd":[1,1,0],"right":[1,1,1],"back":[0,1,0],"left":[1,1,1],"anim":[0,1,0],"turn":[1,0,1]},"log":[["turn",0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":100,"pos":[5000,3000,-2560],"target":[3000,0,256],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[0,1,0],"face":[1,0,1],"arrive":[1,1,0],"permit":[1,1,1],"fwd":[1,0,1],"right":[1,1,1],"back":[1,1,0],"left":[1,1,0],"anim":[1,1,0],"turn":[1,1,0]},"log":[["path",3000,0,256],["turn",0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":0,"heading":100,"pos":[5000,3000,-2560],"target":[3000,0,256],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[0,1,0],"face":[1,0,1],"arrive":[1,1,0],"permit":[1,1,1],"fwd":[1,0,1],"right":[1,1,1],"back":[1,1,0],"left":[1,1,0],"anim":[1,1,0],"turn":[1,1,0]},"log":[["turn",0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":2047,"pos":[-512,3000,256],"target":[256,0,-2560],"angle":300,"elapsed":1,"fallback":0,"script":{"path":[1,0,0],"face":[1,1,0],"arrive":[1,0,1],"permit":[1,1,1],"fwd":[1,1,0],"right":[1,1,0],"back":[1,1,1],"left":[0,1,1],"anim":[1,0,1],"turn":[1,1,1]},"log":[["face",256,-2560]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":2047,"pos":[-512,3000,256],"target":[256,0,-2560],"angle":300,"elapsed":1,"fallback":0,"script":{"path":[1,0,0],"face":[1,1,0],"arrive":[1,0,1],"permit":[1,1,1],"fwd":[1,1,0],"right":[1,1,0],"back":[1,1,1],"left":[0,1,1],"anim":[1,0,1],"turn":[1,1,1]},"log":[["turn",1324]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":700,"pos":[5000,3000,9000],"target":[0,0,400],"angle":1900,"elapsed":0,"fallback":0,"script":{"path":[1,0,1],"face":[1,1,0],"arrive":[0,1,1],"permit":[1,1,1],"fwd":[0,1,0],"right":[1,1,1],"back":[1,1,1],"left":[1,1,1],"anim":[1,1,0],"turn":[1,1,0]},"log":[["path",0,0,400],["face",0,400]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":700,"pos":[5000,3000,9000],"target":[0,0,400],"angle":1900,"elapsed":0,"fallback":0,"script":{"path":[1,0,1],"face":[1,1,0],"arrive":[0,1,1],"permit":[1,1,1],"fwd":[0,1,0],"right":[1,1,1],"back":[1,1,1],"left":[1,1,1],"anim":[1,1,0],"turn":[1,1,0]},"log":[["turn",876]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":2047,"pos":[256,0,-2560],"target":[5000,0,9000],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[1,1,0],"face":[0,1,1],"arrive":[1,1,1],"permit":[1,0,1],"fwd":[1,0,0],"right":[1,1,1],"back":[0,0,0],"left":[1,0,1],"anim":[1,1,0],"turn":[0,1,1]},"log":[["path",5000,0,9000],["face",5000,9000],["arrive",5000,9000],["anim"]],"ret":0,"state":0,"flag":3,"called":[0,0,0,0],"fb":0},
{"m":1,"stale":0,"heading":2047,"pos":[256,0,-2560],"target":[5000,0,9000],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[1,1,0],"face":[0,1,1],"arrive":[1,1,1],"permit":[1,0,1],"fwd":[1,0,0],"right":[1,1,1],"back":[0,0,0],"left":[1,0,1],"anim":[1,1,0],"turn":[0,1,1]},"log":[["turn",0],["arrive",5000,9000],["anim"]],"ret":0,"state":0,"flag":3,"called":[1,1,1,1],"fb":0},
{"m":0,"stale":1,"heading":0,"pos":[5000,0,0],"target":[-512,0,9000],"angle":300,"elapsed":0,"fallback":0,"script":{"path":[1,0,1],"face":[1,0,0],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[0,1,1],"back":[1,1,0],"left":[1,1,0],"anim":[1,1,1],"turn":[1,1,1]},"log":[["path",-512,0,9000]],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":1,"heading":0,"pos":[5000,0,0],"target":[-512,0,9000],"angle":300,"elapsed":0,"fallback":0,"script":{"path":[1,0,1],"face":[1,0,0],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[0,1,1],"back":[1,1,0],"left":[1,1,0],"anim":[1,1,1],"turn":[1,1,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":700,"pos":[-512,0,9000],"target":[0,0,-2560],"angle":300,"elapsed":1,"fallback":0,"script":{"path":[0,1,1],"face":[1,1,0],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[0,1,1],"back":[0,1,0],"left":[1,0,1],"anim":[1,1,1],"turn":[1,0,1]},"log":[["face",0,-2560]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":700,"pos":[-512,0,9000],"target":[0,0,-2560],"angle":300,"elapsed":1,"fallback":0,"script":{"path":[0,1,1],"face":[1,1,0],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[0,1,1],"back":[0,1,0],"left":[1,0,1],"anim":[1,1,1],"turn":[1,0,1]},"log":[["turn",1324]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":1024,"pos":[5000,3000,0],"target":[0,0,9000],"angle":300,"elapsed":5,"fallback":0,"script":{"path":[1,0,0],"face":[1,1,0],"arrive":[0,1,0],"permit":[0,1,1],"fwd":[1,1,1],"right":[0,1,1],"back":[1,1,1],"left":[0,0,0],"anim":[0,1,1],"turn":[0,1,1]},"log":[["face",0,9000]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":1024,"pos":[5000,3000,0],"target":[0,0,9000],"angle":300,"elapsed":5,"fallback":0,"script":{"path":[1,0,0],"face":[1,1,0],"arrive":[0,1,0],"permit":[0,1,1],"fwd":[1,1,1],"right":[0,1,1],"back":[1,1,1],"left":[0,0,0],"anim":[0,1,1],"turn":[0,1,1]},"log":[["turn",1324],["arrive",0,9000],["permit",0,9000]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":0,"pos":[-512,0,9000],"target":[5000,0,9000],"angle":1900,"elapsed":1,"fallback":0,"script":{"path":[1,1,1],"face":[0,0,1],"arrive":[0,1,1],"permit":[0,1,1],"fwd":[1,1,1],"right":[1,0,1],"back":[1,1,1],"left":[1,1,1],"anim":[1,1,0],"turn":[1,0,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":1,"heading":0,"pos":[-512,0,9000],"target":[5000,0,9000],"angle":1900,"elapsed":1,"fallback":0,"script":{"path":[1,1,1],"face":[0,0,1],"arrive":[0,1,1],"permit":[0,1,1],"fwd":[1,1,1],"right":[1,0,1],"back":[1,1,1],"left":[1,1,1],"anim":[1,1,0],"turn":[1,0,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":1500,"pos":[-512,3000,9000],"target":[3000,0,400],"angle":300,"elapsed":0,"fallback":0,"script":{"path":[0,0,1],"face":[0,1,1],"arrive":[1,0,0],"permit":[1,1,1],"fwd":[0,0,1],"right":[0,1,1],"back":[1,1,0],"left":[0,0,1],"anim":[0,1,1],"turn":[0,1,1]},"log":[["path",3000,0,400]],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":1,"heading":1500,"pos":[-512,3000,9000],"target":[3000,0,400],"angle":300,"elapsed":0,"fallback":0,"script":{"path":[0,0,1],"face":[0,1,1],"arrive":[1,0,0],"permit":[1,1,1],"fwd":[0,0,1],"right":[0,1,1],"back":[1,1,0],"left":[0,0,1],"anim":[0,1,1],"turn":[0,1,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":2047,"pos":[5000,0,9000],"target":[256,0,-2560],"angle":300,"elapsed":0,"fallback":1,"script":{"path":[1,1,0],"face":[0,0,0],"arrive":[1,1,1],"permit":[1,0,1],"fwd":[1,0,1],"right":[1,0,0],"back":[1,0,0],"left":[1,1,1],"anim":[1,0,1],"turn":[1,0,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":1,"heading":2047,"pos":[5000,0,9000],"target":[256,0,-2560],"angle":300,"elapsed":0,"fallback":1,"script":{"path":[1,1,0],"face":[0,0,0],"arrive":[1,1,1],"permit":[1,0,1],"fwd":[1,0,1],"right":[1,0,0],"back":[1,0,0],"left":[1,1,1],"anim":[1,0,1],"turn":[1,0,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":0,"stale":0,"heading":1500,"pos":[256,3000,256],"target":[-512,0,9000],"angle":1900,"elapsed":0,"fallback":1,"script":{"path":[1,1,1],"face":[0,1,0],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[1,0,1],"back":[1,1,0],"left":[1,0,1],"anim":[0,1,1],"turn":[1,0,1]},"log":[["turn",876]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":0,"heading":1500,"pos":[256,3000,256],"target":[-512,0,9000],"angle":1900,"elapsed":0,"fallback":1,"script":{"path":[1,1,1],"face":[0,1,0],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[1,0,1],"back":[1,1,0],"left":[1,0,1],"anim":[0,1,1],"turn":[1,0,1]},"log":[["turn",876]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":0,"stale":1,"heading":100,"pos":[256,3000,256],"target":[0,0,0],"angle":1900,"elapsed":0,"fallback":0,"script":{"path":[1,1,1],"face":[0,0,1],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,0,1],"right":[0,1,0],"back":[1,0,1],"left":[0,0,0],"anim":[0,1,1],"turn":[0,1,1]},"log":[["path",0,0,0]],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":1,"heading":100,"pos":[256,3000,256],"target":[0,0,0],"angle":1900,"elapsed":0,"fallback":0,"script":{"path":[1,1,1],"face":[0,0,1],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,0,1],"right":[0,1,0],"back":[1,0,1],"left":[0,0,0],"anim":[0,1,1],"turn":[0,1,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":0,"pos":[-512,0,256],"target":[0,0,-2560],"angle":0,"elapsed":5,"fallback":0,"script":{"path":[1,1,0],"face":[1,1,1],"arrive":[0,1,1],"permit":[1,1,0],"fwd":[1,1,0],"right":[1,1,1],"back":[1,0,1],"left":[1,1,0],"anim":[0,0,1],"turn":[1,0,0]},"log":[["face",0,-2560]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":0,"pos":[-512,0,256],"target":[0,0,-2560],"angle":0,"elapsed":5,"fallback":0,"script":{"path":[1,1,0],"face":[1,1,1],"arrive":[0,1,1],"permit":[1,1,0],"fwd":[1,1,0],"right":[1,1,1],"back":[1,0,1],"left":[1,1,0],"anim":[0,0,1],"turn":[1,0,0]},"log":[["turn",0],["permit",0,-2560],["back",0,-2560]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":2047,"pos":[-512,0,0],"target":[-512,0,0],"angle":0,"elapsed":5,"fallback":0,"script":{"path":[0,1,0],"face":[0,1,1],"arrive":[0,1,0],"permit":[1,1,0],"fwd":[1,1,1],"right":[0,1,1],"back":[1,1,1],"left":[1,1,0],"anim":[0,1,1],"turn":[0,1,0]},"log":[["face",-512,0],["arrive",-512,0],["permit",-512,0],["fwd",-512,0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":2047,"pos":[-512,0,0],"target":[-512,0,0],"angle":0,"elapsed":5,"fallback":0,"script":{"path":[0,1,0],"face":[0,1,1],"arrive":[0,1,0],"permit":[1,1,0],"fwd":[1,1,1],"right":[0,1,1],"back":[1,1,1],"left":[1,1,0],"anim":[0,1,1],"turn":[0,1,0]},"log":[["turn",0],["permit",-512,0],["back",-512,0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":0,"pos":[256,0,-2560],"target":[5000,0,0],"angle":1900,"elapsed":5,"fallback":0,"script":{"path":[0,1,0],"face":[0,1,1],"arrive":[1,1,1],"permit":[1,1,0],"fwd":[1,1,1],"right":[1,0,1],"back":[0,1,1],"left":[1,1,1],"anim":[1,1,1],"turn":[1,1,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":1,"heading":0,"pos":[256,0,-2560],"target":[5000,0,0],"angle":1900,"elapsed":5,"fallback":0,"script":{"path":[0,1,0],"face":[0,1,1],"arrive":[1,1,1],"permit":[1,1,0],"fwd":[1,1,1],"right":[1,0,1],"back":[0,1,1],"left":[1,1,1],"anim":[1,1,1],"turn":[1,1,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":0,"pos":[-512,3000,256],"target":[-512,0,256],"angle":1900,"elapsed":0,"fallback":1,"script":{"path":[1,1,1],"face":[1,1,1],"arrive":[1,1,1],"permit":[0,1,0],"fwd":[1,1,1],"right":[0,1,1],"back":[1,1,1],"left":[0,1,1],"anim":[1,0,0],"turn":[1,1,1]},"log":[["turn",1900],["permit",-512,256]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":0,"heading":0,"pos":[-512,3000,256],"target":[-512,0,256],"angle":1900,"elapsed":0,"fallback":1,"script":{"path":[1,1,1],"face":[1,1,1],"arrive":[1,1,1],"permit":[0,1,0],"fwd":[1,1,1],"right":[0,1,1],"back":[1,1,1],"left":[0,1,1],"anim":[1,0,0],"turn":[1,1,1]},"log":[["turn",1900],["permit",-512,256]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":0,"stale":1,"heading":2047,"pos":[0,3000,0],"target":[5000,0,0],"angle":0,"elapsed":1,"fallback":0,"script":{"path":[1,0,1],"face":[1,1,1],"arrive":[0,1,0],"permit":[0,0,1],"fwd":[0,0,0],"right":[1,1,0],"back":[1,1,0],"left":[1,0,0],"anim":[1,0,1],"turn":[1,0,0]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":1,"heading":2047,"pos":[0,3000,0],"target":[5000,0,0],"angle":0,"elapsed":1,"fallback":0,"script":{"path":[1,0,1],"face":[1,1,1],"arrive":[0,1,0],"permit":[0,0,1],"fwd":[0,0,0],"right":[1,1,0],"back":[1,1,0],"left":[1,0,0],"anim":[1,0,1],"turn":[1,0,0]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":100,"pos":[256,3000,0],"target":[5000,0,-2560],"angle":1900,"elapsed":1,"fallback":0,"script":{"path":[1,0,0],"face":[1,1,0],"arrive":[0,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[1,1,1],"back":[1,1,1],"left":[1,1,0],"anim":[1,0,0],"turn":[0,1,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":1,"heading":100,"pos":[256,3000,0],"target":[5000,0,-2560],"angle":1900,"elapsed":1,"fallback":0,"script":{"path":[1,0,0],"face":[1,1,0],"arrive":[0,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[1,1,1],"back":[1,1,1],"left":[1,1,0],"anim":[1,0,0],"turn":[0,1,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":1500,"pos":[256,3000,9000],"target":[3000,0,256],"angle":300,"elapsed":5,"fallback":0,"script":{"path":[1,1,1],"face":[0,0,1],"arrive":[1,1,1],"permit":[1,0,1],"fwd":[0,1,0],"right":[1,0,0],"back":[1,0,0],"left":[0,1,0],"anim":[1,1,1],"turn":[1,0,0]},"log":[["face",3000,256],["arrive",3000,256],["anim"]],"ret":0,"state":0,"flag":3,"called":[0,0,0,0],"fb":0},
{"m":1,"stale":0,"heading":1500,"pos":[256,3000,9000],"target":[3000,0,256],"angle":300,"elapsed":5,"fallback":0,"script":{"path":[1,1,1],"face":[0,0,1],"arrive":[1,1,1],"permit":[1,0,1],"fwd":[0,1,0],"right":[1,0,0],"back":[1,0,0],"left":[0,1,0],"anim":[1,1,1],"turn":[1,0,0]},"log":[["turn",1324]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":100,"pos":[5000,3000,0],"target":[256,0,-2560],"angle":0,"elapsed":1,"fallback":0,"script":{"path":[1,0,1],"face":[0,1,1],"arrive":[1,1,0],"permit":[1,1,1],"fwd":[1,1,1],"right":[1,0,1],"back":[0,1,0],"left":[1,1,0],"anim":[1,1,1],"turn":[0,1,1]},"log":[["face",256,-2560],["arrive",256,-2560],["anim"]],"ret":0,"state":0,"flag":3,"called":[0,0,0,0],"fb":0},
{"m":1,"stale":0,"heading":100,"pos":[5000,3000,0],"target":[256,0,-2560],"angle":0,"elapsed":1,"fallback":0,"script":{"path":[1,0,1],"face":[0,1,1],"arrive":[1,1,0],"permit":[1,1,1],"fwd":[1,1,1],"right":[1,0,1],"back":[0,1,0],"left":[1,1,0],"anim":[1,1,1],"turn":[0,1,1]},"log":[["turn",0],["permit",256,-2560],["back",256,-2560],["turn",1024]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":2047,"pos":[256,3000,256],"target":[5000,0,256],"angle":0,"elapsed":1,"fallback":1,"script":{"path":[0,1,0],"face":[0,1,1],"arrive":[1,1,1],"permit":[0,0,1],"fwd":[1,1,1],"right":[0,1,1],"back":[0,1,1],"left":[0,1,1],"anim":[1,1,0],"turn":[0,0,1]},"log":[["turn",0],["permit",5000,256]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":0,"heading":2047,"pos":[256,3000,256],"target":[5000,0,256],"angle":0,"elapsed":1,"fallback":1,"script":{"path":[0,1,0],"face":[0,1,1],"arrive":[1,1,1],"permit":[0,0,1],"fwd":[1,1,1],"right":[0,1,1],"back":[0,1,1],"left":[0,1,1],"anim":[1,1,0],"turn":[0,0,1]},"log":[["turn",0],["permit",5000,256]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":0,"stale":0,"heading":1500,"pos":[-512,3000,256],"target":[3000,0,0],"angle":0,"elapsed":0,"fallback":0,"script":{"path":[1,0,1],"face":[1,0,1],"arrive":[0,1,1],"permit":[1,1,1],"fwd":[0,1,0],"right":[1,1,0],"back":[0,0,1],"left":[1,1,0],"anim":[0,1,0],"turn":[0,0,0]},"log":[["path",3000,0,0],["face",3000,0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":1500,"pos":[-512,3000,256],"target":[3000,0,0],"angle":0,"elapsed":0,"fallback":0,"script":{"path":[1,0,1],"face":[1,0,1],"arrive":[0,1,1],"permit":[1,1,1],"fwd":[0,1,0],"right":[1,1,0],"back":[0,0,1],"left":[1,1,0],"anim":[0,1,0],"turn":[0,0,0]},"log":[["turn",1024],["arrive",3000,0],["permit",3000,0],["fwd",3000,0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":100,"pos":[-512,0,256],"target":[256,0,256],"angle":300,"elapsed":5,"fallback":0,"script":{"path":[1,0,0],"face":[1,1,0],"arrive":[0,1,1],"permit":[1,0,1],"fwd":[1,1,0],"right":[1,1,0],"back":[1,1,1],"left":[0,1,1],"anim":[1,1,1],"turn":[1,1,1]},"log":[["face",256,256]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":100,"pos":[-512,0,256],"target":[256,0,256],"angle":300,"elapsed":5,"fallback":0,"script":{"path":[1,0,0],"face":[1,1,0],"arrive":[0,1,1],"permit":[1,0,1],"fwd":[1,1,0],"right":[1,1,0],"back":[1,1,1],"left":[0,1,1],"anim":[1,1,1],"turn":[1,1,1]},"log":[["turn",300],["permit",256,256],["back",256,256]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":100,"pos":[256,3000,0],"target":[256,0,400],"angle":1024,"elapsed":5,"fallback":0,"script":{"path":[0,1,1],"face":[0,1,1],"arrive":[1,1,1],"permit":[1,0,1],"fwd":[1,1,1],"right":[1,0,1],"back":[1,1,0],"left":[0,1,1],"anim":[0,1,1],"turn":[1,0,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":1,"heading":100,"pos":[256,3000,0],"target":[256,0,400],"angle":1024,"elapsed":5,"fallback":0,"script":{"path":[0,1,1],"face":[0,1,1],"arrive":[1,1,1],"permit":[1,0,1],"fwd":[1,1,1],"right":[1,0,1],"back":[1,1,0],"left":[0,1,1],"anim":[0,1,1],"turn":[1,0,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":2047,"pos":[256,3000,0],"target":[3000,0,256],"angle":0,"elapsed":1,"fallback":1,"script":{"path":[1,1,1],"face":[1,1,1],"arrive":[0,1,1],"permit":[0,1,0],"fwd":[1,1,0],"right":[0,1,0],"back":[0,1,1],"left":[0,1,1],"anim":[1,0,0],"turn":[1,1,0]},"log":[["turn",0],["permit",3000,256]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":0,"heading":2047,"pos":[256,3000,0],"target":[3000,0,256],"angle":0,"elapsed":1,"fallback":1,"script":{"path":[1,1,1],"face":[1,1,1],"arrive":[0,1,1],"permit":[0,1,0],"fwd":[1,1,0],"right":[0,1,0],"back":[0,1,1],"left":[0,1,1],"anim":[1,0,0],"turn":[1,1,0]},"log":[["turn",0],["permit",3000,256]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":0,"stale":0,"heading":2047,"pos":[5000,3000,256],"target":[3000,0,400],"angle":0,"elapsed":1,"fallback":0,"script":{"path":[0,1,0],"face":[1,1,1],"arrive":[1,1,1],"permit":[0,1,1],"fwd":[1,1,0],"right":[0,1,1],"back":[1,1,0],"left":[0,0,1],"anim":[0,1,1],"turn":[0,1,1]},"log":[["face",3000,400]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":0,"heading":2047,"pos":[5000,3000,256],"target":[3000,0,400],"angle":0,"elapsed":1,"fallback":0,"script":{"path":[0,1,0],"face":[1,1,1],"arrive":[1,1,1],"permit":[0,1,1],"fwd":[1,1,0],"right":[0,1,1],"back":[1,1,0],"left":[0,0,1],"anim":[0,1,1],"turn":[0,1,1]},"log":[["turn",0],["permit",3000,400]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":2047,"pos":[-512,3000,9000],"target":[3000,0,-2560],"angle":300,"elapsed":0,"fallback":0,"script":{"path":[0,0,0],"face":[1,0,0],"arrive":[1,1,1],"permit":[0,0,1],"fwd":[1,1,1],"right":[1,1,0],"back":[1,1,1],"left":[1,0,1],"anim":[1,1,0],"turn":[1,0,0]},"log":[["path",3000,0,-2560],["turn",1324]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":0,"heading":2047,"pos":[-512,3000,9000],"target":[3000,0,-2560],"angle":300,"elapsed":0,"fallback":0,"script":{"path":[0,0,0],"face":[1,0,0],"arrive":[1,1,1],"permit":[0,0,1],"fwd":[1,1,1],"right":[1,1,0],"back":[1,1,1],"left":[1,0,1],"anim":[1,1,0],"turn":[1,0,0]},"log":[["turn",1324]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":100,"pos":[0,0,-2560],"target":[3000,0,0],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[1,1,0],"face":[0,0,1],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[0,0,0],"back":[1,1,1],"left":[1,1,0],"anim":[1,0,1],"turn":[1,0,1]},"log":[["path",3000,0,0],["face",3000,0],["arrive",3000,0],["anim"]],"ret":0,"state":0,"flag":3,"called":[0,0,0,0],"fb":0},
{"m":1,"stale":0,"heading":100,"pos":[0,0,-2560],"target":[3000,0,0],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[1,1,0],"face":[0,0,1],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[0,0,0],"back":[1,1,1],"left":[1,1,0],"anim":[1,0,1],"turn":[1,0,1]},"log":[["turn",0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":700,"pos":[256,3000,0],"target":[3000,0,9000],"angle":0,"elapsed":1,"fallback":1,"script":{"path":[1,0,1],"face":[1,0,1],"arrive":[0,0,1],"permit":[1,1,0],"fwd":[1,1,1],"right":[0,0,1],"back":[1,1,1],"left":[1,1,1],"anim":[0,0,1],"turn":[1,0,0]},"log":[["turn",1024]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":0,"heading":700,"pos":[256,3000,0],"target":[3000,0,9000],"angle":0,"elapsed":1,"fallback":1,"script":{"path":[1,0,1],"face":[1,0,1],"arrive":[0,0,1],"permit":[1,1,0],"fwd":[1,1,1],"right":[0,0,1],"back":[1,1,1],"left":[1,1,1],"anim":[0,0,1],"turn":[1,0,0]},"log":[["turn",1024]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":0,"stale":0,"heading":100,"pos":[5000,3000,256],"target":[-512,0,-2560],"angle":0,"elapsed":1,"fallback":1,"script":{"path":[0,0,1],"face":[1,0,1],"arrive":[1,1,0],"permit":[0,1,0],"fwd":[0,0,1],"right":[1,1,1],"back":[1,0,1],"left":[1,0,0],"anim":[0,1,1],"turn":[0,0,1]},"log":[["turn",0],["permit",-512,-2560]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":0,"heading":100,"pos":[5000,3000,256],"target":[-512,0,-2560],"angle":0,"elapsed":1,"fallback":1,"script":{"path":[0,0,1],"face":[1,0,1],"arrive":[1,1,0],"permit":[0,1,0],"fwd":[0,0,1],"right":[1,1,1],"back":[1,0,1],"left":[1,0,0],"anim":[0,1,1],"turn":[0,0,1]},"log":[["turn",0],["permit",-512,-2560]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":0,"stale":1,"heading":1500,"pos":[256,0,-2560],"target":[5000,0,0],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[0,1,0],"face":[1,0,1],"arrive":[0,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[1,1,1],"back":[1,0,1],"left":[1,1,1],"anim":[0,0,1],"turn":[0,1,0]},"log":[["path",5000,0,0]],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":1,"heading":1500,"pos":[256,0,-2560],"target":[5000,0,0],"angle":1024,"elapsed":0,"fallback":0,"script":{"path":[0,1,0],"face":[1,0,1],"arrive":[0,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[1,1,1],"back":[1,0,1],"left":[1,1,1],"anim":[0,0,1],"turn":[0,1,0]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":1500,"pos":[0,3000,9000],"target":[3000,0,-2560],"angle":1900,"elapsed":0,"fallback":0,"script":{"path":[1,1,1],"face":[0,1,1],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[1,0,1],"back":[1,0,1],"left":[1,1,1],"anim":[1,1,1],"turn":[1,1,1]},"log":[["path",3000,0,-2560],["face",3000,-2560],["arrive",3000,-2560],["anim"]],"ret":0,"state":0,"flag":3,"called":[0,0,0,0],"fb":0},
{"m":1,"stale":0,"heading":1500,"pos":[0,3000,9000],"target":[3000,0,-2560],"angle":1900,"elapsed":0,"fallback":0,"script":{"path":[1,1,1],"face":[0,1,1],"arrive":[1,1,1],"permit":[1,1,1],"fwd":[1,1,1],"right":[1,0,1],"back":[1,0,1],"left":[1,1,1],"anim":[1,1,1],"turn":[1,1,1]},"log":[["turn",876]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":2047,"pos":[5000,0,256],"target":[0,0,256],"angle":1024,"elapsed":5,"fallback":0,"script":{"path":[0,1,1],"face":[1,0,1],"arrive":[1,1,1],"permit":[1,0,1],"fwd":[0,0,1],"right":[0,1,1],"back":[1,1,1],"left":[1,1,0],"anim":[1,0,1],"turn":[1,0,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":1,"heading":2047,"pos":[5000,0,256],"target":[0,0,256],"angle":1024,"elapsed":5,"fallback":0,"script":{"path":[0,1,1],"face":[1,0,1],"arrive":[1,1,1],"permit":[1,0,1],"fwd":[0,0,1],"right":[0,1,1],"back":[1,1,1],"left":[1,1,0],"anim":[1,0,1],"turn":[1,0,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":0,"pos":[0,3000,9000],"target":[5000,0,9000],"angle":1900,"elapsed":0,"fallback":0,"script":{"path":[1,1,1],"face":[0,1,1],"arrive":[1,0,0],"permit":[0,0,1],"fwd":[1,0,1],"right":[1,0,1],"back":[1,0,1],"left":[1,1,0],"anim":[1,1,1],"turn":[1,1,1]},"log":[["path",5000,0,9000],["face",5000,9000],["arrive",5000,9000],["anim"]],"ret":0,"state":0,"flag":3,"called":[0,0,0,0],"fb":0},
{"m":1,"stale":0,"heading":0,"pos":[0,3000,9000],"target":[5000,0,9000],"angle":1900,"elapsed":0,"fallback":0,"script":{"path":[1,1,1],"face":[0,1,1],"arrive":[1,0,0],"permit":[0,0,1],"fwd":[1,0,1],"right":[1,0,1],"back":[1,0,1],"left":[1,1,0],"anim":[1,1,1],"turn":[1,1,1]},"log":[["turn",1900],["permit",5000,9000]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":1024,"pos":[5000,3000,-2560],"target":[256,0,256],"angle":0,"elapsed":0,"fallback":0,"script":{"path":[1,1,1],"face":[1,1,0],"arrive":[1,0,1],"permit":[1,0,1],"fwd":[0,1,1],"right":[1,0,0],"back":[1,0,0],"left":[1,0,0],"anim":[0,1,0],"turn":[1,1,0]},"log":[["path",256,0,256]],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":1,"heading":1024,"pos":[5000,3000,-2560],"target":[256,0,256],"angle":0,"elapsed":0,"fallback":0,"script":{"path":[1,1,1],"face":[1,1,0],"arrive":[1,0,1],"permit":[1,0,1],"fwd":[0,1,1],"right":[1,0,0],"back":[1,0,0],"left":[1,0,0],"anim":[0,1,0],"turn":[1,1,0]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":1,"heading":0,"pos":[0,3000,256],"target":[-512,0,9000],"angle":1900,"elapsed":0,"fallback":0,"script":{"path":[1,0,0],"face":[0,1,1],"arrive":[0,1,0],"permit":[1,0,1],"fwd":[1,1,0],"right":[1,0,1],"back":[0,0,1],"left":[1,1,1],"anim":[1,0,1],"turn":[1,1,1]},"log":[["path",-512,0,9000]],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":1,"stale":1,"heading":0,"pos":[0,3000,256],"target":[-512,0,9000],"angle":1900,"elapsed":0,"fallback":0,"script":{"path":[1,0,0],"face":[0,1,1],"arrive":[0,1,0],"permit":[1,0,1],"fwd":[1,1,0],"right":[1,0,1],"back":[0,0,1],"left":[1,1,1],"anim":[1,0,1],"turn":[1,1,1]},"log":[],"ret":0,"state":6,"flag":3,"called":[7,7,7,7],"fb":0},
{"m":0,"stale":0,"heading":0,"pos":[0,0,-2560],"target":[256,0,256],"angle":1024,"elapsed":0,"fallback":1,"script":{"path":[1,1,1],"face":[1,1,0],"arrive":[0,1,1],"permit":[0,1,1],"fwd":[1,0,1],"right":[1,1,1],"back":[0,1,0],"left":[1,1,0],"anim":[0,0,0],"turn":[1,0,1]},"log":[["turn",0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":0,"heading":0,"pos":[0,0,-2560],"target":[256,0,256],"angle":1024,"elapsed":0,"fallback":1,"script":{"path":[1,1,1],"face":[1,1,0],"arrive":[0,1,1],"permit":[0,1,1],"fwd":[1,0,1],"right":[1,1,1],"back":[0,1,0],"left":[1,1,0],"anim":[0,0,0],"turn":[1,0,1]},"log":[["turn",0]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":0,"stale":0,"heading":700,"pos":[5000,0,256],"target":[256,0,400],"angle":300,"elapsed":0,"fallback":1,"script":{"path":[0,1,1],"face":[0,1,1],"arrive":[1,1,0],"permit":[0,0,0],"fwd":[0,0,0],"right":[0,0,1],"back":[1,0,1],"left":[1,1,1],"anim":[0,1,1],"turn":[1,0,0]},"log":[["turn",1324]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1},
{"m":1,"stale":0,"heading":700,"pos":[5000,0,256],"target":[256,0,400],"angle":300,"elapsed":0,"fallback":1,"script":{"path":[0,1,1],"face":[0,1,1],"arrive":[1,1,0],"permit":[0,0,0],"fwd":[0,0,0],"right":[0,0,1],"back":[1,0,1],"left":[1,1,1],"anim":[0,1,1],"turn":[1,0,0]},"log":[["turn",1324]],"ret":1,"state":0,"flag":3,"called":[7,7,7,7],"fb":1}
]"#,
        )
        .unwrap();
        for (index, row) in rows.into_iter().enumerate() {
            let mut port = Scripted {
                stale: row.stale != 0,
                heading: row.heading,
                position: [row.pos[0], row.pos[2]],
                script: row.script,
                used: BTreeMap::new(),
                log: Vec::new(),
                called: 7,
                state: 0,
                hook: true,
                masked: false,
                age_over: false,
            };
            let mut state = PickupState {
                fallback: row.fallback != 0,
            };
            let result = if row.m == 0 {
                pickup_mover(&mut port, &mut state, row.elapsed, row.angle, row.target)
            } else {
                plain_mover(&mut port, row.angle, row.target)
            };
            let expected: Vec<(String, Vec<i32>)> = row
                .log
                .into_iter()
                .map(|entry| {
                    (
                        entry[0].as_str().unwrap().to_owned(),
                        entry[1..]
                            .iter()
                            .map(|value| value.as_i64().unwrap() as i32)
                            .collect(),
                    )
                })
                .collect();
            let label = format!("row {index} mover {}", row.m);
            assert_eq!(port.log, expected, "{label}");
            assert_eq!(result, row.ret, "{label}");
            assert_eq!(port.state, row.state, "{label}");
            assert_eq!(i32::from(port.hook) * 2 + 1, row.flag, "{label}");
            // The emulation starts every called flag at 7 and the mover sets all four alike.
            assert_eq!(row.called, [port.called; 4], "{label}");
            if row.m == 0 {
                assert_eq!(u8::from(state.fallback), row.fb, "{label}");
            }
        }
    }

    #[test]
    fn the_lineup_movers_match_the_original_routines_with_logged_helpers() {
        // Rows from the original approach (0) and close (1) movers in an emulator with their leaf
        // helpers stubbed to scripted answers: the scenario (collision-response word and age,
        // player pose, reference pose, angle, offsets, retained fallback, alignment flag), the
        // scripted answers, then the helper calls the original made, its return, return state,
        // alignment flag and fallback flag.
        #[derive(serde::Deserialize)]
        struct Row {
            k: u8,
            mask: u32,
            age: u8,
            heading: u16,
            pos: [i32; 2],
            rp: [i32; 2],
            rh: u16,
            angle: i32,
            off: [i32; 2],
            fb: u8,
            flag: u8,
            script: BTreeMap<String, Vec<i32>>,
            log: Vec<Vec<serde_json::Value>>,
            ret: i32,
            state: i32,
            flag_out: u8,
            fb_out: u32,
        }
        let rows: Vec<Row> = serde_json::from_str(
            r#"[
{"k":0,"mask":0,"age":3,"heading":2047,"pos":[5000,-2560],"rp":[3000,256],"rh":1900,"angle":2100,"off":[256,256],"fb":0,"flag":1,"script":{"roll":[1,1,1,0,0,0],"face":[0,1,0,1,1,1],"arrive":[1,1,0,1,1,1],"permit":[1,0,1,1,0,0],"fwd":[1,0,0,0,1,0],"right":[1,1,1,1,1,0],"back":[0,1,0,1,0,0],"left":[0,0,1,1,1,1],"anim":[0,0,1,1,1,1],"turn":[0,1,0,1,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",3319,427],["turn",928],["anim"]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":256,"age":3,"heading":0,"pos":[0,256],"rp":[3000,0],"rh":1900,"angle":0,"off":[256,-2560],"fb":0,"flag":1,"script":{"roll":[0,1,0,1,1,1],"face":[1,1,1,0,0,0],"arrive":[0,0,1,1,0,1],"permit":[0,1,1,1,0,1],"fwd":[0,1,1,0,1,1],"right":[1,1,1,0,0,0],"back":[1,0,1,1,1,1],"left":[0,1,0,1,1,1],"anim":[0,1,0,0,1,0],"turn":[1,1,0,0,1,1],"err":[1,0,1,0,1,1],"path":[1]},"log":[["arrive",2107,-2413],["turn",876]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":8,"age":3,"heading":0,"pos":[256,-2560],"rp":[256,-2560],"rh":1900,"angle":1900,"off":[2048,-2560],"fb":1,"flag":3,"script":{"roll":[0,1,0,1,1,0],"face":[1,0,1,1,1,1],"arrive":[0,1,1,1,1,1],"permit":[0,0,0,0,1,0],"fwd":[0,0,1,1,0,0],"right":[0,1,1,0,1,1],"back":[1,1,1,0,1,0],"left":[0,0,1,0,1,1],"anim":[1,0,1,1,1,1],"turn":[0,0,1,1,0,1],"err":[1,0,1,0,1,1],"path":[1]},"log":[["face",256,-2560]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":1,"mask":32,"age":3,"heading":2047,"pos":[0,256],"rp":[3000,256],"rh":1024,"angle":1024,"off":[2048,0],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,1,1],"face":[1,1,1,1,1,0],"arrive":[0,1,1,0,1,0],"permit":[1,1,1,0,0,1],"fwd":[0,1,1,1,1,0],"right":[1,1,1,1,1,1],"back":[1,1,1,0,0,0],"left":[0,1,1,0,0,1],"anim":[1,1,1,1,1,1],"turn":[0,1,1,1,1,0],"err":[1,1,0,0,1,0],"path":[1]},"log":[["arrive",5048,256],["turn",1024],["permit",5048,256],["right",5048,256]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":0,"mask":256,"age":3,"heading":2047,"pos":[256,9000],"rp":[-512,256],"rh":1900,"angle":1900,"off":[2048,-2560],"fb":0,"flag":1,"script":{"roll":[0,1,0,1,1,1],"face":[0,0,1,0,0,1],"arrive":[1,1,1,1,1,1],"permit":[0,0,1,1,0,1],"fwd":[1,1,1,1,1,0],"right":[1,0,0,1,1,1],"back":[0,1,1,0,0,1],"left":[1,1,1,1,0,1],"anim":[1,1,1,1,1,1],"turn":[1,1,0,1,1,1],"err":[1,0,0,0,0,1],"path":[1]},"log":[["face",-512,256]],"ret":0,"state":6,"flag_out":1,"fb_out":0},
{"k":1,"mask":8,"age":9,"heading":700,"pos":[0,0],"rp":[0,400],"rh":1900,"angle":1900,"off":[256,0],"fb":0,"flag":1,"script":{"roll":[0,1,0,1,1,0],"face":[1,0,0,1,1,1],"arrive":[0,1,0,1,1,1],"permit":[0,0,1,1,1,0],"fwd":[1,1,0,0,1,1],"right":[1,0,0,0,1,1],"back":[1,1,1,1,1,1],"left":[1,1,0,1,0,1],"anim":[1,1,1,1,0,0],"turn":[1,0,0,1,0,1],"err":[0,1,1,1,0,0],"path":[1]},"log":[["arrive",157,198],["turn",728]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":9,"heading":1024,"pos":[-512,9000],"rp":[256,0],"rh":0,"angle":300,"off":[-512,-2560],"fb":0,"flag":1,"script":{"roll":[1,1,1,0,1,0],"face":[0,1,0,1,1,1],"arrive":[1,1,1,1,0,0],"permit":[1,1,1,1,0,1],"fwd":[0,1,1,0,0,1],"right":[1,1,1,1,0,1],"back":[1,0,1,1,1,0],"left":[1,0,1,1,1,0],"anim":[1,1,1,0,0,1],"turn":[1,1,0,0,1,1],"err":[1,1,1,1,0,0],"path":[1]},"log":[["arrive",1983,-1958],["turn",1324]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":256,"age":9,"heading":100,"pos":[-512,-2560],"rp":[256,0],"rh":1900,"angle":2100,"off":[0,256],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,1,0],"face":[0,0,1,1,1,1],"arrive":[1,1,0,1,0,1],"permit":[0,1,1,0,1,1],"fwd":[1,0,1,1,0,0],"right":[1,1,1,1,1,0],"back":[1,0,0,1,0,1],"left":[0,0,1,1,1,1],"anim":[1,0,1,0,1,1],"turn":[1,1,0,0,1,0],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",330,245],["turn",928]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":8,"age":3,"heading":1500,"pos":[0,-2560],"rp":[0,-2560],"rh":1024,"angle":1900,"off":[0,256],"fb":0,"flag":3,"script":{"roll":[0,1,1,0,1,0],"face":[0,1,0,1,1,1],"arrive":[1,0,1,0,0,1],"permit":[1,1,0,0,1,1],"fwd":[1,1,1,0,0,0],"right":[1,0,1,1,1,0],"back":[0,1,1,1,0,1],"left":[0,1,1,1,1,1],"anim":[1,1,1,1,1,0],"turn":[1,1,0,1,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",-112,-2790],["turn",1900]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":3,"heading":100,"pos":[0,9000],"rp":[256,0],"rh":1024,"angle":1900,"off":[256,256],"fb":1,"flag":3,"script":{"roll":[1,1,1,1,0,0],"face":[1,0,0,0,0,1],"arrive":[0,0,0,1,0,0],"permit":[1,1,0,1,1,0],"fwd":[0,0,1,1,0,1],"right":[0,0,0,0,1,1],"back":[0,0,0,1,1,1],"left":[1,1,0,1,1,1],"anim":[1,1,1,0,1,1],"turn":[0,1,1,1,0,0],"err":[1,0,1,1,1,1],"path":[1]},"log":[["turn",1900],["anim"]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":0,"mask":32,"age":0,"heading":700,"pos":[-512,-2560],"rp":[3000,-2560],"rh":1900,"angle":1024,"off":[0,0],"fb":0,"flag":1,"script":{"roll":[1,1,0,1,0,1],"face":[1,1,0,1,1,1],"arrive":[1,0,0,1,1,1],"permit":[1,1,0,1,1,0],"fwd":[1,1,1,0,0,1],"right":[1,1,1,1,1,0],"back":[0,1,1,0,0,0],"left":[0,0,0,1,0,1],"anim":[0,1,1,0,1,1],"turn":[1,1,0,1,1,1],"err":[0,1,1,1,0,1],"path":[1]},"log":[["face",3000,-2560]],"ret":1,"state":0,"flag_out":1,"fb_out":0},
{"k":1,"mask":2,"age":9,"heading":100,"pos":[-512,0],"rp":[0,256],"rh":1024,"angle":1024,"off":[2048,256],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,0,1],"face":[1,0,0,0,0,1],"arrive":[0,1,1,1,1,1],"permit":[0,0,1,1,1,1],"fwd":[1,0,1,1,0,0],"right":[1,0,0,0,1,0],"back":[1,0,0,1,1,0],"left":[0,1,1,1,1,1],"anim":[1,1,1,1,1,1],"turn":[0,1,1,0,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",2048,512],["turn",1024],["permit",2048,512]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":0,"mask":2,"age":0,"heading":1500,"pos":[-512,-2560],"rp":[256,256],"rh":0,"angle":1024,"off":[256,0],"fb":0,"flag":1,"script":{"roll":[0,1,1,0,1,0],"face":[0,0,0,1,1,1],"arrive":[1,0,0,1,1,1],"permit":[0,1,0,0,0,1],"fwd":[1,1,1,1,0,1],"right":[1,1,1,1,1,0],"back":[0,1,1,0,1,1],"left":[0,0,1,1,1,1],"anim":[1,0,0,1,1,1],"turn":[0,1,1,0,1,1],"err":[1,0,1,0,1,0],"path":[1]},"log":[["face",256,256]],"ret":0,"state":6,"flag_out":1,"fb_out":0},
{"k":1,"mask":8,"age":0,"heading":700,"pos":[256,0],"rp":[0,400],"rh":0,"angle":2100,"off":[2048,-2560],"fb":0,"flag":3,"script":{"roll":[1,1,1,1,0,1],"face":[1,1,0,1,0,0],"arrive":[0,1,1,1,1,1],"permit":[1,1,1,1,0,1],"fwd":[1,1,0,0,1,0],"right":[0,1,1,1,1,1],"back":[0,1,1,1,1,0],"left":[1,1,1,1,1,1],"anim":[1,1,1,1,0,1],"turn":[1,0,1,0,1,1],"err":[0,0,1,0,1,1],"path":[1]},"log":[["arrive",2429,-1802],["turn",1076]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":256,"age":9,"heading":1500,"pos":[-512,256],"rp":[-512,0],"rh":0,"angle":1024,"off":[0,256],"fb":0,"flag":1,"script":{"roll":[1,0,1,1,1,1],"face":[0,1,1,1,1,1],"arrive":[1,1,1,1,1,0],"permit":[1,0,0,1,1,1],"fwd":[1,0,1,1,1,1],"right":[1,1,1,1,1,1],"back":[1,0,0,1,1,1],"left":[1,1,1,1,1,0],"anim":[1,1,0,0,0,0],"turn":[1,1,0,0,1,0],"err":[1,1,1,1,1,1],"path":[1]},"log":[["face",-512,0]],"ret":0,"state":6,"flag_out":1,"fb_out":0},
{"k":1,"mask":0,"age":0,"heading":0,"pos":[5000,256],"rp":[256,400],"rh":1900,"angle":1024,"off":[2048,0],"fb":0,"flag":1,"script":{"roll":[0,1,1,1,1,1],"face":[0,1,1,1,0,1],"arrive":[1,0,1,1,0,0],"permit":[0,1,0,0,1,1],"fwd":[1,1,1,1,1,1],"right":[0,1,1,0,1,1],"back":[1,0,1,1,1,0],"left":[1,0,1,1,1,1],"anim":[1,1,1,1,0,1],"turn":[0,1,0,1,1,1],"err":[1,0,1,1,1,1],"path":[1]},"log":[["arrive",-1584,1298],["turn",1900],["anim"]],"ret":0,"state":0,"flag_out":1,"fb_out":0},
{"k":0,"mask":32,"age":3,"heading":100,"pos":[5000,9000],"rp":[-512,256],"rh":1024,"angle":1024,"off":[-512,-2560],"fb":0,"flag":1,"script":{"roll":[1,0,1,0,1,0],"face":[1,1,1,1,1,1],"arrive":[1,0,1,1,0,0],"permit":[0,1,1,0,0,1],"fwd":[1,1,1,1,1,1],"right":[1,0,1,0,0,1],"back":[1,1,1,0,1,0],"left":[0,1,0,0,0,1],"anim":[1,0,0,1,1,1],"turn":[1,1,1,0,1,0],"err":[1,1,0,1,1,0],"path":[1]},"log":[["face",-512,256]],"ret":1,"state":0,"flag_out":1,"fb_out":0},
{"k":1,"mask":8,"age":0,"heading":1024,"pos":[5000,9000],"rp":[-512,0],"rh":1024,"angle":1900,"off":[-512,0],"fb":1,"flag":1,"script":{"roll":[0,1,1,1,1,1],"face":[1,1,1,0,1,1],"arrive":[1,1,0,1,1,1],"permit":[1,0,1,0,0,1],"fwd":[0,1,0,1,1,0],"right":[1,1,1,0,0,0],"back":[1,1,1,1,0,1],"left":[1,1,1,1,1,0],"anim":[1,1,1,1,0,1],"turn":[1,1,0,0,0,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["turn",1900]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":0,"mask":0,"age":0,"heading":1500,"pos":[0,9000],"rp":[-512,0],"rh":300,"angle":300,"off":[-512,0],"fb":1,"flag":1,"script":{"roll":[1,1,1,0,0,1],"face":[1,1,0,0,0,1],"arrive":[1,1,1,1,1,1],"permit":[1,1,1,0,1,0],"fwd":[1,1,0,1,0,1],"right":[1,1,1,1,1,0],"back":[1,1,1,1,0,1],"left":[0,1,1,0,1,0],"anim":[1,0,1,1,1,1],"turn":[1,0,1,1,1,1],"err":[1,0,0,1,1,1],"path":[1]},"log":[["face",-512,0]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":1,"mask":0,"age":3,"heading":1024,"pos":[0,9000],"rp":[-512,-2560],"rh":1900,"angle":0,"off":[-512,0],"fb":0,"flag":3,"script":{"roll":[1,1,0,0,1,1],"face":[1,1,1,1,1,0],"arrive":[1,1,1,1,1,0],"permit":[1,1,0,1,0,1],"fwd":[1,1,1,0,1,0],"right":[0,1,0,1,1,1],"back":[1,1,1,1,0,0],"left":[1,1,1,1,0,1],"anim":[0,1,1,1,1,1],"turn":[1,1,1,0,0,1],"err":[0,1,1,1,0,0],"path":[1]},"log":[["arrive",-972,-2335],["turn",876]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":8,"age":9,"heading":2047,"pos":[0,0],"rp":[256,256],"rh":1900,"angle":0,"off":[2048,256],"fb":0,"flag":1,"script":{"roll":[1,0,1,0,1,1],"face":[1,1,0,0,1,1],"arrive":[1,1,0,0,1,0],"permit":[0,1,1,1,1,1],"fwd":[0,0,1,1,1,1],"right":[0,1,0,1,1,1],"back":[1,0,1,1,1,1],"left":[1,1,1,1,1,0],"anim":[1,0,0,0,1,1],"turn":[1,1,1,0,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",2209,-412],["turn",876]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":32,"age":0,"heading":1500,"pos":[0,-2560],"rp":[256,400],"rh":1024,"angle":1900,"off":[0,-2560],"fb":1,"flag":1,"script":{"roll":[1,1,1,0,1,1],"face":[1,0,1,1,0,1],"arrive":[1,0,0,1,0,0],"permit":[0,1,0,0,1,1],"fwd":[1,1,1,0,1,1],"right":[1,1,0,0,0,1],"back":[0,1,0,1,1,1],"left":[1,0,0,1,0,1],"anim":[1,1,1,0,1,1],"turn":[0,1,0,1,0,1],"err":[1,1,0,1,1,1],"path":[1]},"log":[["turn",1900],["anim"]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":0,"mask":2,"age":0,"heading":1024,"pos":[5000,9000],"rp":[256,256],"rh":0,"angle":1900,"off":[2048,-2560],"fb":0,"flag":1,"script":{"roll":[1,1,1,0,1,0],"face":[0,1,1,1,0,0],"arrive":[0,0,1,1,1,0],"permit":[1,0,1,1,1,1],"fwd":[1,1,0,0,1,1],"right":[1,0,1,1,1,1],"back":[1,0,1,0,0,1],"left":[1,1,0,1,0,1],"anim":[0,0,0,0,0,1],"turn":[1,1,0,1,1,1],"err":[1,0,1,0,0,1],"path":[1]},"log":[["face",256,256]],"ret":0,"state":6,"flag_out":1,"fb_out":0},
{"k":1,"mask":32,"age":0,"heading":1024,"pos":[0,-2560],"rp":[256,-2560],"rh":1900,"angle":300,"off":[2048,256],"fb":1,"flag":1,"script":{"roll":[1,0,1,1,0,1],"face":[0,0,1,0,1,1],"arrive":[1,1,1,1,1,1],"permit":[0,1,0,1,1,1],"fwd":[1,1,1,1,0,1],"right":[0,1,1,1,1,1],"back":[1,0,1,1,1,0],"left":[1,1,0,1,1,1],"anim":[1,1,1,0,0,0],"turn":[0,0,0,0,0,0],"err":[1,1,1,1,1,1],"path":[1]},"log":[["turn",1176],["anim"]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":0,"mask":2,"age":3,"heading":1024,"pos":[5000,9000],"rp":[256,256],"rh":0,"angle":2100,"off":[256,0],"fb":0,"flag":3,"script":{"roll":[1,0,0,1,0,0],"face":[1,0,1,0,1,1],"arrive":[1,0,0,1,1,1],"permit":[1,0,0,1,0,1],"fwd":[1,1,0,0,1,0],"right":[0,0,1,1,0,0],"back":[1,1,0,1,0,1],"left":[0,0,0,1,1,0],"anim":[0,1,1,1,1,1],"turn":[1,1,1,0,0,0],"err":[0,1,1,1,1,1],"path":[1]},"log":[["face",256,256]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":32,"age":3,"heading":1024,"pos":[256,-2560],"rp":[-512,-2560],"rh":1900,"angle":1024,"off":[2048,0],"fb":0,"flag":1,"script":{"roll":[0,1,1,1,0,1],"face":[1,0,1,1,1,1],"arrive":[0,0,1,1,0,1],"permit":[1,1,1,1,1,1],"fwd":[0,0,0,0,1,0],"right":[1,1,0,0,1,1],"back":[1,1,1,1,1,0],"left":[1,0,0,1,1,1],"anim":[0,1,0,1,1,0],"turn":[0,0,0,0,0,0],"err":[1,1,1,0,0,1],"path":[1]},"log":[["arrive",-2352,-1662],["turn",1900],["permit",-2352,-1662],["right",-2352,-1662]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":0,"mask":2,"age":9,"heading":2047,"pos":[-512,0],"rp":[256,400],"rh":300,"angle":2100,"off":[0,-2560],"fb":0,"flag":3,"script":{"roll":[0,1,0,1,1,0],"face":[1,1,0,0,1,0],"arrive":[1,0,1,0,1,0],"permit":[0,1,1,0,0,1],"fwd":[1,1,1,0,1,1],"right":[1,1,0,0,0,1],"back":[0,0,1,1,1,1],"left":[0,1,1,0,0,1],"anim":[0,1,1,1,0,1],"turn":[1,1,1,1,0,0],"err":[1,1,1,1,1,1],"path":[1]},"log":[["face",256,400]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":8,"age":0,"heading":0,"pos":[0,256],"rp":[-512,0],"rh":1024,"angle":300,"off":[0,-2560],"fb":0,"flag":3,"script":{"roll":[0,0,0,0,1,0],"face":[0,1,1,0,1,0],"arrive":[0,1,1,1,0,1],"permit":[0,0,1,0,1,1],"fwd":[1,1,1,1,0,1],"right":[1,1,1,1,0,0],"back":[1,1,0,1,1,0],"left":[1,1,0,0,0,1],"anim":[1,0,1,0,1,1],"turn":[0,1,1,1,1,1],"err":[0,1,1,1,0,1],"path":[1]},"log":[["arrive",-2549,1550],["turn",300],["permit",-2549,1550]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":32,"age":0,"heading":700,"pos":[-512,9000],"rp":[3000,400],"rh":1024,"angle":300,"off":[256,256],"fb":1,"flag":1,"script":{"roll":[0,1,1,1,0,1],"face":[0,0,1,1,1,1],"arrive":[0,1,1,1,1,0],"permit":[0,0,1,0,0,1],"fwd":[0,1,1,0,1,1],"right":[0,0,1,1,0,1],"back":[1,1,1,1,1,1],"left":[1,0,1,1,1,0],"anim":[1,1,0,0,1,1],"turn":[1,0,1,1,1,1],"err":[0,1,0,1,1,0],"path":[1]},"log":[["face",3000,400]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":1,"mask":32,"age":3,"heading":2047,"pos":[256,9000],"rp":[3000,-2560],"rh":0,"angle":2100,"off":[-512,256],"fb":0,"flag":1,"script":{"roll":[0,1,1,1,1,0],"face":[1,1,0,1,0,0],"arrive":[0,1,1,1,1,1],"permit":[0,1,0,1,1,1],"fwd":[0,1,1,1,1,1],"right":[0,0,0,0,1,1],"back":[1,1,1,1,1,1],"left":[1,1,1,0,0,0],"anim":[0,1,0,1,0,1],"turn":[1,1,1,1,0,1],"err":[1,1,1,1,0,1],"path":[1]},"log":[["arrive",2454,-2389],["turn",1076]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":2,"age":0,"heading":1500,"pos":[5000,256],"rp":[256,-2560],"rh":300,"angle":1024,"off":[0,1000],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,1,1],"face":[1,0,1,1,1,1],"arrive":[1,0,0,1,1,0],"permit":[0,1,0,0,0,1],"fwd":[1,1,1,0,1,0],"right":[1,1,0,1,1,0],"back":[1,0,0,0,1,1],"left":[0,1,1,1,1,1],"anim":[0,0,0,1,1,0],"turn":[1,0,1,0,1,1],"err":[1,1,0,1,1,0],"path":[1]},"log":[["face",256,-2560]],"ret":1,"state":0,"flag_out":1,"fb_out":0},
{"k":1,"mask":2,"age":0,"heading":1500,"pos":[256,256],"rp":[3000,-2560],"rh":0,"angle":1024,"off":[2048,-2560],"fb":0,"flag":1,"script":{"roll":[1,1,0,1,1,1],"face":[0,0,1,1,1,1],"arrive":[0,1,1,0,0,1],"permit":[0,1,0,1,1,1],"fwd":[1,1,0,1,1,0],"right":[1,1,1,0,1,0],"back":[1,1,1,1,0,1],"left":[1,0,1,1,1,1],"anim":[1,1,1,0,0,0],"turn":[1,1,0,0,1,1],"err":[0,0,1,1,0,1],"path":[1]},"log":[["arrive",952,0],["turn",0]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":8,"age":0,"heading":1500,"pos":[256,256],"rp":[3000,-2560],"rh":1024,"angle":0,"off":[256,1000],"fb":0,"flag":1,"script":{"roll":[1,0,1,1,1,0],"face":[1,1,1,1,1,1],"arrive":[0,1,0,1,1,1],"permit":[1,0,1,1,0,0],"fwd":[1,1,1,1,1,1],"right":[0,1,1,1,1,1],"back":[1,1,1,1,1,0],"left":[0,1,0,1,1,0],"anim":[0,1,1,1,0,0],"turn":[0,1,1,0,1,0],"err":[0,1,1,0,1,1],"path":[1]},"log":[["arrive",2744,-3560],["face",2744,-3560]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":32,"age":0,"heading":700,"pos":[5000,256],"rp":[3000,400],"rh":0,"angle":2100,"off":[0,256],"fb":0,"flag":1,"script":{"roll":[0,1,1,0,0,1],"face":[1,0,1,1,1,1],"arrive":[1,1,0,1,0,1],"permit":[1,0,0,1,1,1],"fwd":[0,1,1,1,0,1],"right":[0,0,1,0,0,1],"back":[1,1,0,1,0,0],"left":[0,0,0,1,1,1],"anim":[0,0,1,1,1,1],"turn":[1,1,0,1,1,0],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",2959,653],["turn",1076]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":9,"heading":1024,"pos":[5000,9000],"rp":[0,256],"rh":300,"angle":0,"off":[-512,-2560],"fb":1,"flag":3,"script":{"roll":[1,1,1,0,0,1],"face":[1,0,0,1,0,1],"arrive":[1,1,0,0,0,1],"permit":[1,0,1,1,1,0],"fwd":[1,1,1,1,0,1],"right":[0,1,0,1,1,1],"back":[1,1,1,1,0,1],"left":[1,1,0,1,1,0],"anim":[1,1,1,1,0,0],"turn":[1,0,1,1,0,0],"err":[0,1,0,1,1,0],"path":[1]},"log":[["face",0,256]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":1,"mask":256,"age":0,"heading":700,"pos":[0,-2560],"rp":[3000,400],"rh":1900,"angle":300,"off":[256,256],"fb":0,"flag":3,"script":{"roll":[0,1,0,1,0,0],"face":[1,1,1,0,0,0],"arrive":[0,1,1,1,1,1],"permit":[1,1,0,1,1,0],"fwd":[0,0,1,1,1,1],"right":[1,0,1,0,1,1],"back":[0,1,1,1,0,1],"left":[1,1,1,1,0,0],"anim":[1,1,1,1,0,1],"turn":[1,1,1,1,1,1],"err":[0,1,0,1,1,1],"path":[1]},"log":[["arrive",3114,744],["turn",1176]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":0,"heading":700,"pos":[5000,9000],"rp":[-512,-2560],"rh":1024,"angle":0,"off":[-512,0],"fb":1,"flag":3,"script":{"roll":[1,1,1,1,1,1],"face":[0,1,1,0,0,0],"arrive":[1,1,1,1,1,1],"permit":[0,1,1,1,1,1],"fwd":[1,1,1,1,1,1],"right":[1,0,1,0,0,1],"back":[1,1,1,0,1,1],"left":[0,0,1,1,1,1],"anim":[1,1,0,1,1,1],"turn":[1,1,0,1,1,0],"err":[1,0,0,1,1,1],"path":[1]},"log":[["face",-512,-2560]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":1,"mask":0,"age":0,"heading":1500,"pos":[256,0],"rp":[-512,400],"rh":0,"angle":1024,"off":[-512,256],"fb":1,"flag":1,"script":{"roll":[1,1,0,1,1,1],"face":[1,1,0,0,0,0],"arrive":[0,0,1,0,1,0],"permit":[0,0,0,1,0,1],"fwd":[1,1,0,0,1,0],"right":[0,1,1,0,1,1],"back":[1,1,1,1,1,1],"left":[1,0,1,1,0,1],"anim":[0,1,1,1,0,0],"turn":[1,0,1,1,1,0],"err":[1,0,1,0,1,1],"path":[1]},"log":[["turn",0]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":0,"mask":256,"age":3,"heading":100,"pos":[-512,256],"rp":[0,400],"rh":0,"angle":2100,"off":[2048,0],"fb":0,"flag":3,"script":{"roll":[0,1,1,1,1,1],"face":[0,1,1,1,1,0],"arrive":[0,1,1,1,1,1],"permit":[1,1,0,1,1,1],"fwd":[0,1,1,1,1,1],"right":[0,1,0,1,1,0],"back":[0,0,1,0,1,1],"left":[0,1,0,1,1,1],"anim":[0,1,1,1,1,1],"turn":[1,0,1,1,1,1],"err":[1,0,0,0,1,1],"path":[1]},"log":[["face",0,400]],"ret":0,"state":6,"flag_out":1,"fb_out":0},
{"k":1,"mask":256,"age":9,"heading":1500,"pos":[256,0],"rp":[3000,-2560],"rh":300,"angle":2100,"off":[0,256],"fb":0,"flag":1,"script":{"roll":[1,0,1,0,0,1],"face":[1,0,1,1,1,0],"arrive":[0,0,1,1,1,1],"permit":[1,0,1,0,0,0],"fwd":[1,1,1,0,0,0],"right":[0,0,1,1,1,1],"back":[0,1,1,1,1,1],"left":[1,1,1,1,0,1],"anim":[1,0,1,1,1,1],"turn":[1,1,1,0,1,0],"err":[0,1,1,1,1,1],"path":[1]},"log":[["arrive",2774,-2439],["turn",1376]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":256,"age":0,"heading":700,"pos":[256,-2560],"rp":[3000,-2560],"rh":1024,"angle":300,"off":[0,256],"fb":1,"flag":1,"script":{"roll":[1,0,1,0,1,1],"face":[0,1,1,1,0,0],"arrive":[1,1,0,1,1,0],"permit":[0,0,1,1,1,1],"fwd":[1,0,1,1,0,0],"right":[1,1,1,1,1,1],"back":[1,1,1,1,0,1],"left":[1,1,1,1,1,0],"anim":[1,1,0,1,1,1],"turn":[1,0,1,1,1,1],"err":[1,0,1,0,1,1],"path":[1]},"log":[["face",3000,-2560]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":1,"mask":2,"age":9,"heading":100,"pos":[-512,0],"rp":[3000,-2560],"rh":1900,"angle":1024,"off":[256,0],"fb":0,"flag":3,"script":{"roll":[0,1,1,0,1,0],"face":[0,1,1,1,1,0],"arrive":[0,1,1,1,1,0],"permit":[1,1,0,1,0,0],"fwd":[0,0,1,1,1,0],"right":[0,1,1,1,0,1],"back":[0,0,1,1,0,0],"left":[1,0,1,1,0,0],"anim":[1,1,1,0,1,1],"turn":[1,0,0,1,1,1],"err":[1,1,1,1,0,0],"path":[1]},"log":[["arrive",2770,-2448],["turn",1900]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":256,"age":3,"heading":2047,"pos":[5000,9000],"rp":[-512,256],"rh":1900,"angle":300,"off":[-512,1000],"fb":0,"flag":3,"script":{"roll":[1,0,1,0,0,0],"face":[1,1,1,0,1,1],"arrive":[0,0,1,1,1,1],"permit":[1,1,0,0,1,1],"fwd":[1,1,1,1,1,1],"right":[0,1,1,1,1,0],"back":[0,1,0,0,1,1],"left":[1,1,1,1,1,1],"anim":[1,1,0,0,1,1],"turn":[0,0,1,1,1,0],"err":[0,0,1,1,1,1],"path":[1]},"log":[["face",-512,256]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":2,"age":9,"heading":700,"pos":[5000,256],"rp":[256,400],"rh":1024,"angle":300,"off":[0,256],"fb":1,"flag":3,"script":{"roll":[1,1,1,0,0,1],"face":[1,1,1,1,1,0],"arrive":[1,0,1,1,0,1],"permit":[1,1,1,0,1,0],"fwd":[0,0,0,1,0,0],"right":[1,0,1,0,1,0],"back":[0,1,1,1,0,1],"left":[1,1,0,1,1,0],"anim":[0,1,1,1,1,1],"turn":[1,1,1,1,1,0],"err":[0,1,1,1,1,1],"path":[1]},"log":[["turn",300]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":0,"mask":32,"age":3,"heading":1024,"pos":[5000,-2560],"rp":[3000,-2560],"rh":1024,"angle":0,"off":[256,1000],"fb":0,"flag":3,"script":{"roll":[0,1,0,1,0,0],"face":[1,1,1,0,1,1],"arrive":[1,0,1,1,1,0],"permit":[1,1,1,1,1,1],"fwd":[0,1,1,1,0,1],"right":[1,1,0,1,1,1],"back":[1,1,1,1,1,1],"left":[1,1,1,0,1,1],"anim":[1,0,0,0,0,0],"turn":[0,1,1,0,1,1],"err":[1,0,1,0,0,0],"path":[1]},"log":[["face",3000,-2560]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":9,"heading":2047,"pos":[-512,0],"rp":[3000,256],"rh":1900,"angle":300,"off":[0,-2560],"fb":1,"flag":3,"script":{"roll":[1,1,0,1,1,0],"face":[1,0,1,0,1,1],"arrive":[1,1,0,1,1,1],"permit":[1,1,1,1,0,1],"fwd":[1,1,1,0,0,1],"right":[0,1,0,1,1,0],"back":[0,0,0,1,0,1],"left":[1,1,1,1,0,0],"anim":[1,1,1,1,1,0],"turn":[1,1,0,1,0,0],"err":[0,1,0,0,1,1],"path":[1]},"log":[["turn",1176]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":0,"mask":2,"age":0,"heading":1024,"pos":[0,256],"rp":[-512,256],"rh":1900,"angle":1900,"off":[0,256],"fb":1,"flag":3,"script":{"roll":[0,1,1,1,1,1],"face":[1,1,1,0,1,0],"arrive":[0,1,1,0,0,1],"permit":[1,1,1,0,1,1],"fwd":[1,1,1,1,0,1],"right":[0,1,0,0,1,0],"back":[1,1,1,0,0,1],"left":[0,1,1,1,1,0],"anim":[1,1,1,1,1,1],"turn":[0,0,0,1,0,1],"err":[0,0,0,1,1,0],"path":[1]},"log":[["face",-512,256]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":1,"mask":8,"age":0,"heading":2047,"pos":[-512,-2560],"rp":[-512,256],"rh":1024,"angle":1024,"off":[0,0],"fb":1,"flag":1,"script":{"roll":[0,0,1,1,1,1],"face":[1,1,1,1,1,1],"arrive":[1,1,1,0,1,1],"permit":[1,1,1,0,0,0],"fwd":[1,1,1,1,0,0],"right":[0,1,1,1,0,1],"back":[1,1,0,0,1,1],"left":[0,0,1,1,1,1],"anim":[1,1,1,1,1,1],"turn":[1,0,1,1,1,1],"err":[1,1,0,1,1,1],"path":[1]},"log":[["turn",1024]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":0,"mask":8,"age":9,"heading":0,"pos":[-512,256],"rp":[256,256],"rh":300,"angle":0,"off":[0,256],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,1,1],"face":[1,1,1,1,1,1],"arrive":[1,0,1,0,0,1],"permit":[0,0,1,1,1,0],"fwd":[1,0,1,1,1,1],"right":[1,1,1,1,1,0],"back":[1,1,0,0,1,0],"left":[1,0,1,1,1,0],"anim":[1,1,1,0,1,1],"turn":[1,0,1,1,1,0],"err":[0,0,1,0,1,1],"path":[1]},"log":[["arrive",52,411],["turn",1324]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":3,"heading":1500,"pos":[-512,0],"rp":[-512,256],"rh":0,"angle":2100,"off":[2048,256],"fb":0,"flag":3,"script":{"roll":[1,1,0,1,1,0],"face":[0,0,1,1,0,0],"arrive":[1,0,0,1,1,1],"permit":[1,0,1,1,0,0],"fwd":[0,0,0,0,1,1],"right":[1,0,0,1,0,1],"back":[1,0,0,1,0,0],"left":[1,1,0,1,0,1],"anim":[1,1,1,0,1,0],"turn":[1,0,1,0,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",1469,834],["turn",1076]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":2,"age":3,"heading":700,"pos":[5000,0],"rp":[256,-2560],"rh":0,"angle":1900,"off":[2048,256],"fb":0,"flag":3,"script":{"roll":[1,1,1,1,0,0],"face":[1,1,1,1,0,1],"arrive":[0,1,1,0,1,1],"permit":[0,1,1,1,0,1],"fwd":[0,0,0,1,0,0],"right":[1,1,1,1,0,1],"back":[1,1,1,1,0,1],"left":[1,0,0,1,1,0],"anim":[1,1,0,0,1,1],"turn":[1,0,1,1,1,1],"err":[1,1,1,1,1,0],"path":[1]},"log":[["face",256,-2560]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":0,"heading":100,"pos":[-512,-2560],"rp":[3000,400],"rh":0,"angle":0,"off":[256,256],"fb":0,"flag":3,"script":{"roll":[1,1,1,1,1,1],"face":[1,0,1,0,1,1],"arrive":[1,0,1,1,1,0],"permit":[1,1,1,1,1,1],"fwd":[1,1,1,1,0,1],"right":[1,1,1,1,0,1],"back":[0,1,0,1,1,1],"left":[1,1,1,1,1,0],"anim":[0,1,0,1,1,0],"turn":[1,1,1,1,0,0],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",3256,656],["turn",1024]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":256,"age":9,"heading":1024,"pos":[0,9000],"rp":[256,0],"rh":0,"angle":2100,"off":[-512,0],"fb":1,"flag":1,"script":{"roll":[0,1,0,1,1,1],"face":[1,1,1,0,1,0],"arrive":[0,0,1,1,0,0],"permit":[0,1,1,1,1,1],"fwd":[1,0,1,1,0,0],"right":[1,1,0,0,1,0],"back":[0,0,1,1,1,1],"left":[1,1,0,0,1,1],"anim":[1,0,1,1,0,1],"turn":[0,1,0,0,1,0],"err":[1,1,1,1,1,1],"path":[1]},"log":[["face",256,0]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":1,"mask":8,"age":9,"heading":100,"pos":[0,256],"rp":[0,-2560],"rh":0,"angle":0,"off":[-512,-2560],"fb":0,"flag":1,"script":{"roll":[0,0,1,1,0,1],"face":[1,0,0,1,0,0],"arrive":[1,0,1,1,0,1],"permit":[0,1,1,1,1,0],"fwd":[0,0,0,0,1,1],"right":[0,1,1,0,1,1],"back":[0,0,0,1,1,1],"left":[1,1,1,1,1,1],"anim":[1,1,1,1,1,1],"turn":[1,1,1,1,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",-512,-5120],["turn",1024]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":2,"age":9,"heading":0,"pos":[-512,256],"rp":[3000,-2560],"rh":1024,"angle":0,"off":[256,1000],"fb":0,"flag":3,"script":{"roll":[1,0,1,1,1,0],"face":[1,1,1,0,1,1],"arrive":[1,0,1,1,0,1],"permit":[1,1,0,1,1,1],"fwd":[0,0,0,1,0,1],"right":[1,1,0,0,0,1],"back":[1,1,0,1,0,1],"left":[1,0,0,1,0,1],"anim":[0,1,1,1,0,1],"turn":[1,1,1,0,1,1],"err":[1,1,0,1,0,1],"path":[1]},"log":[["face",3000,-2560]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":2,"age":9,"heading":700,"pos":[5000,256],"rp":[-512,0],"rh":0,"angle":1024,"off":[2048,-2560],"fb":0,"flag":3,"script":{"roll":[1,1,1,1,0,1],"face":[1,0,0,1,0,0],"arrive":[1,1,1,1,0,1],"permit":[1,1,1,1,1,1],"fwd":[1,0,1,1,0,1],"right":[1,1,1,1,1,1],"back":[0,1,1,1,1,0],"left":[1,0,0,1,1,0],"anim":[1,1,1,1,0,1],"turn":[0,1,1,1,0,1],"err":[1,1,1,1,1,0],"path":[1]},"log":[["arrive",-2560,2560],["turn",0],["anim"]],"ret":0,"state":0,"flag_out":1,"fb_out":0},
{"k":0,"mask":32,"age":0,"heading":1500,"pos":[0,0],"rp":[-512,400],"rh":0,"angle":2100,"off":[0,0],"fb":1,"flag":1,"script":{"roll":[0,0,0,0,1,0],"face":[1,0,0,0,0,1],"arrive":[1,0,1,1,1,1],"permit":[1,0,1,0,1,1],"fwd":[0,1,1,0,0,0],"right":[0,1,1,1,1,1],"back":[0,1,1,1,1,0],"left":[0,1,1,1,0,1],"anim":[0,1,1,1,0,0],"turn":[0,1,1,1,0,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["face",-512,400]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":1,"mask":256,"age":9,"heading":2047,"pos":[256,9000],"rp":[256,0],"rh":1024,"angle":300,"off":[0,0],"fb":0,"flag":3,"script":{"roll":[1,0,1,1,1,0],"face":[0,1,1,1,1,1],"arrive":[1,1,1,0,1,1],"permit":[1,1,1,1,1,1],"fwd":[1,1,1,1,0,1],"right":[1,1,1,0,1,1],"back":[1,1,1,1,1,1],"left":[1,1,0,1,1,1],"anim":[1,1,1,1,1,1],"turn":[0,0,0,1,1,1],"err":[1,1,1,1,1,0],"path":[1]},"log":[["arrive",256,0],["turn",300],["anim"]],"ret":0,"state":0,"flag_out":1,"fb_out":0},
{"k":0,"mask":0,"age":3,"heading":2047,"pos":[0,0],"rp":[0,-2560],"rh":0,"angle":0,"off":[256,-2560],"fb":0,"flag":1,"script":{"roll":[0,1,1,1,1,1],"face":[1,1,0,1,0,1],"arrive":[1,0,1,1,1,1],"permit":[1,1,0,1,1,1],"fwd":[1,1,0,1,0,1],"right":[0,0,1,0,0,1],"back":[0,0,0,1,1,0],"left":[0,1,0,1,0,0],"anim":[1,1,0,1,0,1],"turn":[1,1,1,0,1,0],"err":[0,1,0,1,1,1],"path":[1]},"log":[["arrive",256,-5120],["turn",1024]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":0,"heading":2047,"pos":[-512,9000],"rp":[3000,0],"rh":0,"angle":1024,"off":[0,1000],"fb":0,"flag":1,"script":{"roll":[1,1,1,0,1,1],"face":[1,1,1,0,0,1],"arrive":[1,1,1,0,0,1],"permit":[1,1,1,0,1,1],"fwd":[1,1,1,1,1,1],"right":[1,0,1,1,1,0],"back":[1,1,1,0,1,1],"left":[1,0,0,1,1,1],"anim":[1,1,1,1,1,0],"turn":[1,0,1,1,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",3000,-1000],["turn",0]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":256,"age":0,"heading":100,"pos":[5000,0],"rp":[-512,0],"rh":0,"angle":1024,"off":[-512,0],"fb":0,"flag":3,"script":{"roll":[1,1,0,1,1,0],"face":[1,0,1,1,1,1],"arrive":[1,1,1,1,1,0],"permit":[1,1,0,1,1,1],"fwd":[1,0,1,0,0,1],"right":[0,1,1,1,1,1],"back":[1,1,1,0,0,0],"left":[1,1,1,0,0,1],"anim":[0,1,1,1,1,1],"turn":[0,1,0,0,1,0],"err":[1,0,1,1,0,1],"path":[1]},"log":[["face",-512,0]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":0,"heading":0,"pos":[-512,-2560],"rp":[-512,256],"rh":1900,"angle":1024,"off":[256,-2560],"fb":0,"flag":1,"script":{"roll":[0,1,0,0,0,0],"face":[1,0,1,0,1,1],"arrive":[1,1,1,0,1,1],"permit":[0,0,0,1,0,0],"fwd":[1,1,0,1,1,0],"right":[0,0,1,1,0,1],"back":[1,0,0,0,1,1],"left":[1,0,1,1,1,1],"anim":[0,1,1,1,0,0],"turn":[1,1,1,1,1,0],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",381,2669],["turn",1900]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":8,"age":0,"heading":2047,"pos":[5000,0],"rp":[0,400],"rh":1024,"angle":0,"off":[256,0],"fb":0,"flag":3,"script":{"roll":[0,1,0,1,0,1],"face":[1,0,1,1,1,0],"arrive":[1,1,0,0,1,1],"permit":[1,1,0,1,1,1],"fwd":[0,1,1,1,1,1],"right":[1,1,1,0,1,1],"back":[1,0,1,1,0,1],"left":[1,1,1,1,1,0],"anim":[1,1,1,1,1,1],"turn":[1,0,1,1,1,0],"err":[0,1,1,1,0,1],"path":[1]},"log":[["arrive",-256,400],["turn",0]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":256,"age":9,"heading":2047,"pos":[-512,0],"rp":[256,256],"rh":0,"angle":300,"off":[-512,1000],"fb":1,"flag":3,"script":{"roll":[0,1,0,0,0,1],"face":[1,1,1,1,1,1],"arrive":[1,0,1,1,1,1],"permit":[1,1,0,1,1,1],"fwd":[1,1,0,1,1,0],"right":[0,1,0,1,1,1],"back":[1,1,0,1,0,1],"left":[1,1,1,1,1,1],"anim":[1,1,1,0,0,1],"turn":[1,1,0,1,0,1],"err":[0,0,1,1,0,1],"path":[1]},"log":[["turn",1324]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":0,"mask":256,"age":9,"heading":700,"pos":[-512,256],"rp":[256,256],"rh":0,"angle":300,"off":[2048,1000],"fb":0,"flag":3,"script":{"roll":[0,1,1,0,1,0],"face":[1,0,1,0,0,0],"arrive":[1,0,0,0,1,0],"permit":[1,0,1,1,1,1],"fwd":[1,0,1,1,0,1],"right":[1,0,1,0,1,0],"back":[1,1,1,1,1,0],"left":[1,1,1,1,1,1],"anim":[1,1,1,0,1,1],"turn":[1,1,1,0,1,0],"err":[1,1,1,0,1,0],"path":[1]},"log":[["face",256,256]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":256,"age":3,"heading":1500,"pos":[5000,0],"rp":[256,0],"rh":0,"angle":1900,"off":[2048,-2560],"fb":0,"flag":1,"script":{"roll":[0,1,1,1,1,1],"face":[1,1,0,0,1,1],"arrive":[0,1,0,0,1,0],"permit":[1,1,1,1,0,0],"fwd":[1,1,0,1,0,1],"right":[1,1,0,1,0,0],"back":[1,1,0,0,1,0],"left":[1,1,1,0,0,1],"anim":[1,0,0,1,1,1],"turn":[0,1,0,1,1,0],"err":[1,0,1,1,1,1],"path":[1]},"log":[["arrive",974,-3199],["turn",876],["permit",974,-3199],["back",974,-3199]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":0,"mask":256,"age":0,"heading":100,"pos":[-512,0],"rp":[-512,256],"rh":300,"angle":0,"off":[0,256],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,0,1],"face":[1,0,0,1,1,1],"arrive":[1,0,0,1,1,1],"permit":[1,1,0,1,0,0],"fwd":[0,0,1,1,0,1],"right":[1,1,0,0,1,0],"back":[1,0,1,1,1,1],"left":[0,0,0,0,1,1],"anim":[1,1,1,0,1,0],"turn":[0,0,1,0,1,1],"err":[1,1,0,0,0,1],"path":[1]},"log":[["face",-512,256]],"ret":1,"state":0,"flag_out":1,"fb_out":0},
{"k":1,"mask":0,"age":9,"heading":1024,"pos":[-512,256],"rp":[256,400],"rh":300,"angle":0,"off":[-512,0],"fb":0,"flag":1,"script":{"roll":[1,0,1,1,1,1],"face":[1,1,0,0,1,1],"arrive":[1,1,1,1,0,1],"permit":[1,1,1,0,1,1],"fwd":[0,0,1,1,0,1],"right":[1,1,0,1,1,0],"back":[1,1,0,1,1,1],"left":[0,1,1,1,1,1],"anim":[0,1,0,1,1,1],"turn":[1,0,0,1,1,0],"err":[0,0,0,1,0,0],"path":[1]},"log":[["arrive",-54,-7],["turn",1324]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":3,"heading":2047,"pos":[-512,0],"rp":[3000,400],"rh":0,"angle":0,"off":[0,1000],"fb":0,"flag":3,"script":{"roll":[1,1,1,1,1,1],"face":[1,1,1,1,1,0],"arrive":[1,1,1,0,1,1],"permit":[1,1,1,0,1,1],"fwd":[1,1,0,1,1,1],"right":[1,1,0,0,1,1],"back":[0,1,1,0,1,1],"left":[0,1,1,1,1,1],"anim":[1,0,0,1,1,1],"turn":[1,1,0,0,1,0],"err":[0,0,0,0,0,0],"path":[1]},"log":[["arrive",3000,1400],["turn",1024]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":256,"age":9,"heading":1024,"pos":[0,0],"rp":[0,256],"rh":1024,"angle":2100,"off":[0,-2560],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,1,0],"face":[1,1,1,1,1,1],"arrive":[1,0,1,1,0,0],"permit":[1,1,0,1,0,0],"fwd":[0,0,0,0,1,1],"right":[0,1,1,1,1,1],"back":[0,1,1,0,0,1],"left":[1,1,0,1,1,0],"anim":[1,1,1,0,1,1],"turn":[0,0,0,0,1,0],"err":[0,1,1,0,0,0],"path":[1]},"log":[["arrive",-407,2783],["turn",52],["anim"]],"ret":0,"state":0,"flag_out":1,"fb_out":0},
{"k":0,"mask":0,"age":3,"heading":1024,"pos":[5000,-2560],"rp":[0,0],"rh":1900,"angle":0,"off":[-512,256],"fb":0,"flag":1,"script":{"roll":[0,1,1,1,1,1],"face":[1,1,0,0,0,1],"arrive":[0,1,1,1,0,1],"permit":[0,1,0,0,1,0],"fwd":[1,0,0,1,1,0],"right":[1,1,1,1,1,1],"back":[0,0,1,1,1,1],"left":[1,1,1,0,1,1],"anim":[1,1,1,1,1,0],"turn":[1,1,1,1,0,0],"err":[1,1,1,0,0,0],"path":[1]},"log":[["arrive",-348,455],["face",-348,455]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":9,"heading":700,"pos":[-512,9000],"rp":[256,0],"rh":1900,"angle":300,"off":[-512,0],"fb":0,"flag":1,"script":{"roll":[1,0,1,1,0,1],"face":[1,1,1,1,1,0],"arrive":[0,0,1,1,1,0],"permit":[0,1,1,0,1,1],"fwd":[0,1,1,1,1,1],"right":[0,1,1,1,0,1],"back":[0,1,1,0,0,1],"left":[1,1,0,0,1,1],"anim":[1,1,0,1,1,0],"turn":[1,1,1,1,1,1],"err":[0,1,0,0,1,1],"path":[1]},"log":[["arrive",-201,-230],["turn",1176]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":0,"heading":700,"pos":[0,256],"rp":[0,400],"rh":1024,"angle":0,"off":[-512,0],"fb":0,"flag":1,"script":{"roll":[1,1,1,0,1,1],"face":[1,0,0,1,1,1],"arrive":[0,1,1,1,1,0],"permit":[0,1,1,1,1,1],"fwd":[1,0,0,0,1,1],"right":[0,1,0,1,0,1],"back":[0,1,1,1,1,1],"left":[1,1,1,1,0,1],"anim":[1,1,1,1,1,1],"turn":[1,1,0,1,1,1],"err":[1,0,1,1,0,0],"path":[1]},"log":[["arrive",512,400],["face",512,400]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":3,"heading":1500,"pos":[0,256],"rp":[-512,400],"rh":300,"angle":1900,"off":[256,0],"fb":0,"flag":3,"script":{"roll":[0,1,1,1,1,0],"face":[0,0,1,1,1,1],"arrive":[1,1,0,1,1,0],"permit":[0,0,0,0,0,1],"fwd":[1,1,1,0,1,1],"right":[1,0,1,0,1,1],"back":[1,1,1,1,1,1],"left":[1,1,1,0,1,1],"anim":[0,0,0,1,1,0],"turn":[0,1,0,1,0,1],"err":[0,0,1,1,1,1],"path":[1]},"log":[["arrive",-283,515],["turn",1176],["anim"]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":9,"heading":1500,"pos":[5000,-2560],"rp":[256,0],"rh":1024,"angle":2100,"off":[256,-2560],"fb":0,"flag":1,"script":{"roll":[1,0,0,1,1,1],"face":[0,1,1,0,1,1],"arrive":[0,1,0,1,0,0],"permit":[0,1,0,1,0,0],"fwd":[1,1,0,1,1,0],"right":[1,1,1,1,1,0],"back":[1,1,1,0,0,1],"left":[0,0,1,1,1,1],"anim":[1,1,1,1,0,1],"turn":[0,1,1,1,1,0],"err":[1,0,1,1,1,1],"path":[1]},"log":[["arrive",-403,2487],["face",-403,2487],["permit",-403,2487]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":32,"age":3,"heading":2047,"pos":[0,9000],"rp":[-512,-2560],"rh":0,"angle":1024,"off":[2048,0],"fb":1,"flag":3,"script":{"roll":[0,1,1,0,1,1],"face":[1,1,1,1,0,1],"arrive":[1,1,1,1,1,0],"permit":[1,0,1,1,1,1],"fwd":[1,0,1,1,0,1],"right":[1,1,1,1,1,1],"back":[0,0,1,1,1,0],"left":[1,0,1,1,1,0],"anim":[1,0,1,1,1,1],"turn":[0,1,1,1,0,0],"err":[1,0,1,1,1,1],"path":[1]},"log":[["turn",0],["anim"]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":0,"mask":32,"age":0,"heading":2047,"pos":[-512,0],"rp":[0,0],"rh":1900,"angle":0,"off":[2048,0],"fb":0,"flag":3,"script":{"roll":[0,1,0,1,1,1],"face":[1,1,1,0,1,1],"arrive":[1,1,0,1,1,0],"permit":[1,0,0,1,1,0],"fwd":[1,1,0,0,0,0],"right":[0,1,1,1,1,1],"back":[0,1,0,1,1,0],"left":[1,1,1,0,1,1],"anim":[1,1,0,0,1,0],"turn":[1,0,0,1,0,1],"err":[1,0,1,1,1,1],"path":[1]},"log":[["face",0,0]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":0,"heading":700,"pos":[-512,256],"rp":[-512,400],"rh":0,"angle":300,"off":[256,1000],"fb":1,"flag":1,"script":{"roll":[0,1,0,1,1,0],"face":[1,1,1,1,1,1],"arrive":[0,1,0,1,1,0],"permit":[1,1,0,0,0,1],"fwd":[0,1,1,1,0,1],"right":[1,0,1,1,1,1],"back":[0,1,0,0,0,1],"left":[1,0,1,1,1,1],"anim":[0,1,1,0,0,0],"turn":[1,1,1,1,1,0],"err":[1,1,1,1,0,1],"path":[1]},"log":[["turn",1324]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":0,"mask":256,"age":0,"heading":1024,"pos":[0,256],"rp":[-512,0],"rh":1900,"angle":1024,"off":[-512,1000],"fb":1,"flag":1,"script":{"roll":[1,1,1,1,1,1],"face":[0,0,1,0,0,1],"arrive":[1,1,0,1,1,1],"permit":[1,1,1,0,0,0],"fwd":[1,1,0,1,1,1],"right":[1,1,1,1,1,1],"back":[1,1,0,1,1,1],"left":[0,0,1,1,1,1],"anim":[0,0,1,0,1,1],"turn":[1,0,1,0,1,0],"err":[0,0,0,1,1,1],"path":[1]},"log":[["face",-512,0]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":1,"mask":0,"age":0,"heading":100,"pos":[-512,9000],"rp":[3000,256],"rh":1900,"angle":0,"off":[2048,1000],"fb":0,"flag":3,"script":{"roll":[1,1,1,1,0,0],"face":[1,0,1,0,0,1],"arrive":[1,0,1,1,1,1],"permit":[1,1,1,0,0,1],"fwd":[1,1,1,1,0,1],"right":[0,0,1,1,1,1],"back":[1,1,1,1,1,1],"left":[1,1,1,0,1,0],"anim":[0,1,1,0,1,1],"turn":[1,1,0,1,1,0],"err":[1,0,1,1,0,1],"path":[1]},"log":[["arrive",5279,256],["turn",876]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":0,"heading":2047,"pos":[0,9000],"rp":[3000,400],"rh":0,"angle":0,"off":[-512,-2560],"fb":1,"flag":1,"script":{"roll":[0,0,0,1,1,0],"face":[1,1,1,0,0,0],"arrive":[1,1,1,0,1,1],"permit":[0,1,0,1,1,1],"fwd":[1,1,0,1,0,0],"right":[1,1,1,0,1,1],"back":[1,1,0,1,1,1],"left":[1,1,1,1,1,1],"anim":[1,0,1,1,1,1],"turn":[1,0,1,1,1,0],"err":[0,1,1,1,0,1],"path":[1]},"log":[["face",3000,400]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":1,"mask":2,"age":3,"heading":0,"pos":[0,9000],"rp":[-512,-2560],"rh":1900,"angle":1024,"off":[0,256],"fb":0,"flag":1,"script":{"roll":[1,1,0,1,1,0],"face":[1,1,1,1,1,0],"arrive":[0,0,1,1,1,0],"permit":[1,0,1,1,0,1],"fwd":[0,1,1,1,0,1],"right":[1,1,0,0,0,0],"back":[0,1,0,1,1,1],"left":[1,0,0,0,1,1],"anim":[0,1,1,1,0,0],"turn":[1,1,1,1,1,0],"err":[1,0,0,1,1,1],"path":[1]},"log":[["arrive",-624,-2790],["turn",1900]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":3,"heading":1024,"pos":[256,9000],"rp":[-512,-2560],"rh":0,"angle":2100,"off":[0,0],"fb":1,"flag":1,"script":{"roll":[1,0,1,1,1,1],"face":[1,0,1,1,1,0],"arrive":[0,1,1,0,1,0],"permit":[0,1,0,0,1,1],"fwd":[0,1,0,0,0,1],"right":[0,1,1,0,0,1],"back":[1,1,1,0,1,0],"left":[0,1,1,1,0,0],"anim":[0,1,0,1,1,1],"turn":[1,1,1,1,0,1],"err":[1,1,0,1,1,1],"path":[1]},"log":[["face",-512,-2560]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":1,"mask":8,"age":3,"heading":0,"pos":[256,9000],"rp":[-512,0],"rh":1900,"angle":1024,"off":[256,-2560],"fb":0,"flag":3,"script":{"roll":[0,0,0,0,0,0],"face":[1,1,1,1,1,0],"arrive":[1,0,1,1,0,0],"permit":[1,0,1,1,1,0],"fwd":[1,1,0,1,0,1],"right":[0,0,1,1,1,1],"back":[1,1,1,1,1,1],"left":[1,0,1,0,1,1],"anim":[1,1,1,0,1,0],"turn":[1,1,1,0,1,1],"err":[0,1,1,0,1,1],"path":[1]},"log":[["arrive",381,2413],["turn",1900]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":9,"heading":1024,"pos":[0,9000],"rp":[256,400],"rh":300,"angle":2100,"off":[0,1000],"fb":0,"flag":3,"script":{"roll":[0,1,1,1,1,1],"face":[1,1,1,0,1,1],"arrive":[1,1,1,1,0,1],"permit":[0,1,1,0,1,1],"fwd":[0,1,1,0,0,1],"right":[0,1,1,1,0,0],"back":[0,1,0,1,1,1],"left":[1,0,1,1,0,0],"anim":[1,1,1,1,1,0],"turn":[1,1,0,1,1,0],"err":[1,1,1,0,1,1],"path":[1]},"log":[["arrive",-626,871],["turn",1376]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":256,"age":0,"heading":700,"pos":[256,9000],"rp":[-512,0],"rh":1024,"angle":1900,"off":[256,-2560],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,1,0],"face":[1,1,0,0,1,0],"arrive":[1,1,0,1,0,1],"permit":[0,0,1,0,0,1],"fwd":[1,1,0,1,1,1],"right":[1,1,1,1,1,0],"back":[0,0,1,0,1,0],"left":[1,1,1,1,1,1],"anim":[1,0,1,1,1,1],"turn":[1,1,1,1,1,0],"err":[1,1,0,1,0,0],"path":[1]},"log":[["arrive",381,2413],["turn",1900]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":0,"heading":2047,"pos":[256,-2560],"rp":[-512,400],"rh":0,"angle":1900,"off":[2048,256],"fb":0,"flag":1,"script":{"roll":[0,0,1,1,0,1],"face":[0,1,0,0,1,1],"arrive":[0,1,1,1,1,1],"permit":[1,1,0,0,1,1],"fwd":[1,1,0,1,1,1],"right":[1,1,0,1,1,1],"back":[0,1,0,1,1,0],"left":[0,1,1,1,1,1],"anim":[0,1,1,1,1,0],"turn":[1,1,1,0,1,0],"err":[1,1,1,1,0,0],"path":[1]},"log":[["arrive",1441,-268],["face",1441,-268],["permit",1441,-268],["fwd",1441,-268]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":3,"heading":0,"pos":[256,9000],"rp":[256,0],"rh":1900,"angle":1900,"off":[256,-2560],"fb":0,"flag":1,"script":{"roll":[0,0,1,0,0,1],"face":[1,1,1,1,1,1],"arrive":[1,1,0,0,1,1],"permit":[1,1,0,1,1,1],"fwd":[1,0,0,1,0,0],"right":[1,1,0,0,1,1],"back":[1,1,0,1,1,0],"left":[0,0,1,1,1,0],"anim":[1,0,0,1,1,1],"turn":[1,0,1,1,0,1],"err":[1,0,0,1,1,0],"path":[1]},"log":[["arrive",-1605,-1777],["turn",728]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":32,"age":9,"heading":100,"pos":[0,256],"rp":[3000,400],"rh":0,"angle":1024,"off":[256,0],"fb":1,"flag":3,"script":{"roll":[1,1,1,1,1,1],"face":[0,1,1,0,1,0],"arrive":[1,1,0,1,1,1],"permit":[1,1,1,0,1,1],"fwd":[0,1,1,1,1,1],"right":[0,1,1,1,1,0],"back":[1,1,1,1,0,1],"left":[1,1,1,0,1,0],"anim":[0,1,1,0,1,1],"turn":[1,0,0,1,1,0],"err":[1,0,1,0,1,1],"path":[1]},"log":[["face",3000,400]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":1,"mask":8,"age":3,"heading":1024,"pos":[-512,-2560],"rp":[256,256],"rh":300,"angle":0,"off":[-512,-2560],"fb":0,"flag":3,"script":{"roll":[0,1,1,1,1,1],"face":[1,1,1,0,0,1],"arrive":[0,1,0,1,1,1],"permit":[1,0,0,0,1,0],"fwd":[1,0,0,0,1,1],"right":[0,0,0,1,0,1],"back":[0,1,1,1,1,0],"left":[1,1,0,1,1,1],"anim":[1,0,1,1,1,0],"turn":[0,1,0,0,0,1],"err":[1,1,0,0,0,0],"path":[1]},"log":[["arrive",1983,-1702],["turn",1324],["permit",1983,-1702],["left",1983,-1702]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":9,"heading":700,"pos":[0,256],"rp":[-512,0],"rh":1900,"angle":1900,"off":[-512,256],"fb":1,"flag":3,"script":{"roll":[1,0,1,1,1,1],"face":[1,1,1,1,1,1],"arrive":[0,1,1,1,1,1],"permit":[1,1,0,1,0,0],"fwd":[1,0,1,1,1,1],"right":[1,1,0,0,0,0],"back":[1,1,0,1,1,1],"left":[0,1,1,1,1,0],"anim":[1,1,1,1,0,1],"turn":[1,1,1,1,1,1],"err":[0,0,0,0,1,1],"path":[1]},"log":[["face",-512,0]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":1,"mask":32,"age":3,"heading":700,"pos":[256,9000],"rp":[0,0],"rh":1900,"angle":2100,"off":[0,1000],"fb":1,"flag":3,"script":{"roll":[1,0,1,1,1,0],"face":[0,1,0,1,1,1],"arrive":[1,0,1,1,1,1],"permit":[1,1,0,0,1,1],"fwd":[1,1,0,1,1,1],"right":[1,1,1,1,0,1],"back":[1,1,0,1,1,1],"left":[1,0,1,1,0,1],"anim":[1,1,0,1,1,1],"turn":[1,0,1,1,0,1],"err":[1,1,1,1,0,0],"path":[1]},"log":[["turn",928]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":0,"mask":8,"age":9,"heading":1024,"pos":[-512,9000],"rp":[256,0],"rh":1024,"angle":2100,"off":[0,0],"fb":0,"flag":1,"script":{"roll":[1,0,0,1,0,1],"face":[1,1,1,1,1,0],"arrive":[0,1,1,1,1,0],"permit":[1,0,0,1,1,1],"fwd":[1,1,1,1,1,1],"right":[0,1,1,1,0,0],"back":[1,0,1,0,0,1],"left":[0,1,0,1,1,0],"anim":[0,0,1,1,0,1],"turn":[1,1,0,0,1,0],"err":[1,0,1,0,1,1],"path":[1]},"log":[["arrive",256,0],["face",256,0]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":0,"heading":1024,"pos":[5000,-2560],"rp":[256,-2560],"rh":1900,"angle":2100,"off":[-512,1000],"fb":0,"flag":1,"script":{"roll":[0,1,1,0,0,1],"face":[1,1,0,1,1,0],"arrive":[1,1,0,0,1,1],"permit":[0,0,1,1,0,1],"fwd":[1,1,1,1,1,0],"right":[1,1,0,0,0,1],"back":[1,0,1,0,1,1],"left":[0,1,0,1,1,1],"anim":[1,1,1,0,1,1],"turn":[1,1,1,1,0,1],"err":[1,0,0,1,1,0],"path":[1]},"log":[["arrive",56,-1454],["turn",928]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":256,"age":3,"heading":700,"pos":[0,-2560],"rp":[-512,400],"rh":1024,"angle":1024,"off":[0,0],"fb":1,"flag":3,"script":{"roll":[1,1,1,0,0,1],"face":[0,1,1,1,1,0],"arrive":[1,1,0,1,1,0],"permit":[1,1,1,0,1,1],"fwd":[1,0,1,1,1,0],"right":[0,0,0,1,1,1],"back":[1,0,1,1,1,1],"left":[1,1,1,1,0,0],"anim":[1,0,1,1,0,1],"turn":[1,1,1,1,1,0],"err":[1,1,1,1,0,1],"path":[1]},"log":[["face",-512,400]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":1,"mask":256,"age":9,"heading":2047,"pos":[-512,256],"rp":[256,256],"rh":0,"angle":1024,"off":[2048,1000],"fb":0,"flag":3,"script":{"roll":[0,1,1,1,1,1],"face":[1,1,1,1,1,1],"arrive":[1,1,1,1,1,0],"permit":[1,0,1,1,1,1],"fwd":[1,1,1,0,1,1],"right":[0,1,1,1,1,1],"back":[1,0,1,1,1,1],"left":[0,1,1,0,0,1],"anim":[1,1,1,0,1,1],"turn":[0,1,0,0,1,1],"err":[0,0,1,1,0,1],"path":[1]},"log":[["arrive",-1792,-744],["turn",0],["anim"]],"ret":0,"state":0,"flag_out":1,"fb_out":0},
{"k":0,"mask":32,"age":3,"heading":0,"pos":[-512,-2560],"rp":[3000,256],"rh":0,"angle":0,"off":[2048,256],"fb":0,"flag":1,"script":{"roll":[0,0,1,1,0,1],"face":[1,0,1,0,1,1],"arrive":[0,1,1,1,0,0],"permit":[0,1,1,1,0,0],"fwd":[1,1,0,0,1,0],"right":[1,0,1,0,1,1],"back":[1,0,0,0,1,1],"left":[0,1,1,1,0,1],"anim":[1,1,1,1,1,1],"turn":[1,1,0,0,1,1],"err":[1,1,1,0,0,0],"path":[1]},"log":[["face",3000,256]],"ret":1,"state":0,"flag_out":1,"fb_out":0},
{"k":1,"mask":0,"age":0,"heading":1024,"pos":[-512,9000],"rp":[3000,0],"rh":1024,"angle":1024,"off":[-512,0],"fb":0,"flag":1,"script":{"roll":[1,0,1,1,1,1],"face":[1,1,1,0,0,1],"arrive":[1,1,1,1,0,0],"permit":[1,1,1,1,1,1],"fwd":[1,1,1,0,1,0],"right":[0,0,0,0,1,0],"back":[1,1,1,0,1,1],"left":[1,0,1,0,1,1],"anim":[1,0,0,1,1,1],"turn":[1,1,0,1,1,1],"err":[0,0,1,1,1,0],"path":[1]},"log":[["arrive",2488,0],["turn",1024]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":8,"age":0,"heading":700,"pos":[5000,-2560],"rp":[0,-2560],"rh":1024,"angle":2100,"off":[2048,1000],"fb":1,"flag":1,"script":{"roll":[1,0,1,0,1,0],"face":[1,1,1,1,1,1],"arrive":[0,1,0,0,1,1],"permit":[1,1,0,0,1,1],"fwd":[0,1,0,1,1,1],"right":[1,1,1,0,1,0],"back":[0,1,1,0,1,0],"left":[1,1,1,0,1,1],"anim":[1,1,1,0,1,1],"turn":[1,0,1,1,1,1],"err":[1,1,0,0,1,0],"path":[1]},"log":[["face",0,-2560]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":1,"mask":2,"age":9,"heading":0,"pos":[256,0],"rp":[3000,-2560],"rh":0,"angle":0,"off":[2048,256],"fb":0,"flag":3,"script":{"roll":[1,1,0,1,1,1],"face":[1,1,1,1,1,0],"arrive":[1,0,1,0,0,0],"permit":[1,0,0,0,1,1],"fwd":[1,0,1,0,1,1],"right":[1,1,0,0,1,1],"back":[1,1,1,1,0,1],"left":[1,1,0,0,0,1],"anim":[1,1,1,1,0,1],"turn":[1,0,0,0,0,0],"err":[0,0,1,0,1,0],"path":[1]},"log":[["arrive",5048,-2304],["turn",1024]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":9,"heading":700,"pos":[0,9000],"rp":[3000,-2560],"rh":300,"angle":300,"off":[256,-2560],"fb":1,"flag":3,"script":{"roll":[0,1,1,0,1,1],"face":[1,1,1,1,1,1],"arrive":[1,0,1,1,1,1],"permit":[1,1,0,1,1,1],"fwd":[1,0,0,1,0,1],"right":[1,0,1,1,1,1],"back":[0,0,1,1,0,0],"left":[1,0,0,1,0,1],"anim":[0,1,0,1,1,0],"turn":[1,1,1,1,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["face",3000,-2560]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":1,"mask":2,"age":9,"heading":100,"pos":[5000,0],"rp":[-512,0],"rh":0,"angle":1900,"off":[0,0],"fb":1,"flag":1,"script":{"roll":[1,1,0,1,1,1],"face":[0,0,1,1,1,0],"arrive":[1,1,1,1,0,1],"permit":[1,1,0,0,0,0],"fwd":[1,1,1,1,0,0],"right":[0,1,1,1,1,1],"back":[0,1,1,0,1,1],"left":[1,1,0,0,1,0],"anim":[1,1,1,1,1,1],"turn":[1,1,1,1,0,1],"err":[1,1,1,0,0,0],"path":[1]},"log":[["turn",876]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":0,"mask":0,"age":0,"heading":0,"pos":[256,0],"rp":[3000,0],"rh":300,"angle":1900,"off":[0,1000],"fb":0,"flag":3,"script":{"roll":[1,1,0,1,1,1],"face":[1,1,0,1,1,1],"arrive":[1,1,1,1,0,1],"permit":[1,1,1,0,1,1],"fwd":[1,1,1,1,0,0],"right":[0,1,0,1,1,0],"back":[1,1,0,1,1,1],"left":[0,1,1,1,0,1],"anim":[0,1,0,1,0,0],"turn":[0,0,0,0,0,1],"err":[0,1,1,0,1,1],"path":[1]},"log":[["arrive",2550,893],["turn",1176],["anim"]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":3,"heading":1024,"pos":[0,-2560],"rp":[256,256],"rh":0,"angle":1900,"off":[2048,-2560],"fb":1,"flag":1,"script":{"roll":[1,0,1,0,1,1],"face":[1,1,1,1,0,1],"arrive":[1,0,1,0,0,0],"permit":[0,0,1,0,0,1],"fwd":[1,1,1,1,0,1],"right":[0,1,1,1,1,1],"back":[1,1,1,1,1,0],"left":[0,0,1,1,1,1],"anim":[1,1,0,1,1,1],"turn":[0,1,1,1,0,1],"err":[0,1,1,0,1,0],"path":[1]},"log":[["turn",876],["anim"]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":0,"mask":8,"age":0,"heading":700,"pos":[0,256],"rp":[-512,-2560],"rh":1900,"angle":300,"off":[0,1000],"fb":0,"flag":1,"script":{"roll":[0,1,1,0,1,1],"face":[0,1,1,1,1,1],"arrive":[0,1,1,1,1,1],"permit":[0,0,1,1,1,1],"fwd":[0,1,1,1,1,1],"right":[1,0,0,1,0,1],"back":[0,1,1,0,0,0],"left":[0,1,0,1,0,1],"anim":[0,0,1,1,0,1],"turn":[1,0,1,1,0,1],"err":[0,1,1,1,1,1],"path":[1]},"log":[["arrive",-962,-1667],["face",-962,-1667],["permit",-962,-1667]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":256,"age":3,"heading":700,"pos":[-512,9000],"rp":[0,-2560],"rh":1900,"angle":1900,"off":[256,1000],"fb":0,"flag":3,"script":{"roll":[1,1,0,0,1,1],"face":[1,0,1,0,0,1],"arrive":[0,0,1,1,1,1],"permit":[1,1,1,1,0,0],"fwd":[1,1,1,0,0,0],"right":[0,0,1,1,1,1],"back":[1,0,0,1,1,0],"left":[1,0,1,0,1,0],"anim":[0,1,1,1,1,1],"turn":[1,1,1,1,1,1],"err":[0,0,1,1,1,0],"path":[1]},"log":[["arrive",946,-2147],["turn",728]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":8,"age":9,"heading":1500,"pos":[5000,256],"rp":[0,400],"rh":1024,"angle":300,"off":[2048,0],"fb":1,"flag":3,"script":{"roll":[0,1,1,0,0,1],"face":[1,1,1,1,0,1],"arrive":[1,1,0,0,1,1],"permit":[1,1,1,1,0,1],"fwd":[0,0,1,1,1,1],"right":[1,1,0,1,1,0],"back":[1,1,1,1,1,1],"left":[0,0,1,0,1,1],"anim":[1,1,1,1,1,0],"turn":[0,1,1,1,0,1],"err":[1,1,1,1,1,0],"path":[1]},"log":[["face",0,400]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":1,"mask":2,"age":3,"heading":2047,"pos":[256,-2560],"rp":[-512,256],"rh":1024,"angle":1024,"off":[256,1000],"fb":0,"flag":3,"script":{"roll":[1,1,0,1,0,1],"face":[1,1,1,1,0,1],"arrive":[1,1,1,0,1,0],"permit":[1,1,0,1,1,1],"fwd":[1,1,1,1,1,1],"right":[1,1,1,1,0,1],"back":[1,1,0,0,1,1],"left":[1,1,0,1,1,1],"anim":[1,1,1,1,1,1],"turn":[0,1,0,1,1,1],"err":[1,0,1,1,0,1],"path":[1]},"log":[["arrive",-256,1256],["turn",1024],["anim"]],"ret":0,"state":0,"flag_out":1,"fb_out":0},
{"k":0,"mask":8,"age":3,"heading":1024,"pos":[256,9000],"rp":[256,400],"rh":300,"angle":1900,"off":[0,-2560],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,1,0],"face":[1,0,1,0,0,1],"arrive":[0,1,1,1,1,1],"permit":[0,1,0,1,1,1],"fwd":[1,0,1,1,1,0],"right":[1,1,0,1,0,0],"back":[1,1,1,0,1,1],"left":[0,0,1,0,0,1],"anim":[1,0,1,0,1,0],"turn":[1,1,0,0,1,0],"err":[1,1,1,0,1,1],"path":[1]},"log":[["arrive",1407,-1887],["face",1407,-1887]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":9,"heading":0,"pos":[0,256],"rp":[0,-2560],"rh":0,"angle":0,"off":[-512,1000],"fb":0,"flag":1,"script":{"roll":[1,1,1,0,0,1],"face":[1,1,1,1,1,0],"arrive":[0,0,0,0,0,1],"permit":[1,0,0,1,1,1],"fwd":[0,0,1,1,0,1],"right":[1,0,1,1,1,1],"back":[0,0,0,0,1,1],"left":[1,0,1,1,1,1],"anim":[1,0,1,0,0,1],"turn":[1,1,0,1,1,1],"err":[0,0,1,0,1,1],"path":[1]},"log":[["arrive",-512,-1560],["turn",1024]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":32,"age":0,"heading":700,"pos":[5000,0],"rp":[256,256],"rh":0,"angle":1024,"off":[-512,1000],"fb":0,"flag":3,"script":{"roll":[1,1,0,0,0,1],"face":[1,1,1,1,0,0],"arrive":[1,0,0,0,1,1],"permit":[1,0,1,0,0,1],"fwd":[1,0,0,1,0,0],"right":[1,0,0,1,1,1],"back":[0,1,1,1,1,1],"left":[0,0,1,0,1,1],"anim":[1,1,1,1,1,1],"turn":[0,1,0,1,1,1],"err":[1,0,1,0,0,0],"path":[1]},"log":[["face",256,256]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":256,"age":9,"heading":0,"pos":[5000,9000],"rp":[3000,256],"rh":0,"angle":300,"off":[256,1000],"fb":0,"flag":3,"script":{"roll":[1,1,1,0,1,1],"face":[0,1,1,0,0,0],"arrive":[1,0,0,1,0,1],"permit":[1,0,0,0,1,0],"fwd":[1,0,0,0,1,1],"right":[0,0,0,1,0,0],"back":[0,1,1,1,1,0],"left":[1,0,1,1,0,1],"anim":[1,1,1,1,1,1],"turn":[0,1,0,1,0,1],"err":[0,0,1,0,1,1],"path":[1]},"log":[["arrive",2359,1065],["turn",1324],["anim"]],"ret":0,"state":0,"flag_out":1,"fb_out":0},
{"k":0,"mask":8,"age":3,"heading":700,"pos":[0,-2560],"rp":[0,256],"rh":300,"angle":1900,"off":[2048,0],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,0,0],"face":[1,0,1,1,1,0],"arrive":[1,1,0,1,1,1],"permit":[1,1,1,0,1,0],"fwd":[1,1,1,1,1,1],"right":[1,1,0,1,0,1],"back":[1,1,1,0,1,0],"left":[1,1,1,1,1,0],"anim":[0,0,1,1,0,1],"turn":[1,1,0,1,0,0],"err":[1,0,1,0,1,0],"path":[1]},"log":[["arrive",1829,1177],["turn",1176]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":256,"age":3,"heading":700,"pos":[256,256],"rp":[3000,256],"rh":300,"angle":2100,"off":[-512,0],"fb":1,"flag":1,"script":{"roll":[1,0,1,1,1,1],"face":[1,1,1,0,1,1],"arrive":[0,1,0,1,1,1],"permit":[0,1,1,0,1,1],"fwd":[1,1,1,1,1,1],"right":[1,1,1,0,0,0],"back":[1,1,1,0,0,0],"left":[1,1,1,1,0,1],"anim":[1,0,0,1,1,1],"turn":[1,1,0,0,1,1],"err":[1,1,1,1,1,0],"path":[1]},"log":[["turn",1376]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":0,"mask":256,"age":0,"heading":1500,"pos":[-512,9000],"rp":[-512,-2560],"rh":300,"angle":0,"off":[0,0],"fb":1,"flag":1,"script":{"roll":[1,1,0,1,1,1],"face":[1,1,1,1,1,0],"arrive":[1,1,1,1,1,1],"permit":[1,1,1,0,1,1],"fwd":[1,1,1,1,1,1],"right":[1,1,0,0,1,0],"back":[1,1,0,1,1,0],"left":[0,0,1,1,1,1],"anim":[0,1,1,0,1,1],"turn":[0,1,1,1,1,1],"err":[1,1,0,1,0,1],"path":[1]},"log":[["face",-512,-2560]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":1,"mask":32,"age":0,"heading":2047,"pos":[-512,0],"rp":[3000,256],"rh":0,"angle":0,"off":[0,256],"fb":0,"flag":3,"script":{"roll":[1,0,1,1,1,1],"face":[1,1,1,1,1,1],"arrive":[1,0,1,1,1,0],"permit":[0,1,1,1,1,1],"fwd":[1,1,0,0,1,0],"right":[1,1,1,1,1,0],"back":[0,1,1,1,1,1],"left":[1,1,1,1,0,0],"anim":[0,0,1,1,0,0],"turn":[0,0,0,1,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",3000,512],["turn",1024],["anim"]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":8,"age":9,"heading":2047,"pos":[256,0],"rp":[3000,400],"rh":1900,"angle":2100,"off":[2048,0],"fb":1,"flag":1,"script":{"roll":[1,1,0,1,1,0],"face":[0,0,1,0,1,1],"arrive":[1,0,1,1,0,0],"permit":[1,1,1,1,0,1],"fwd":[1,1,1,1,1,0],"right":[1,1,0,0,1,1],"back":[1,0,1,1,1,0],"left":[1,0,0,0,1,1],"anim":[1,1,0,1,0,0],"turn":[0,0,1,0,1,1],"err":[0,1,1,0,1,1],"path":[1]},"log":[["face",3000,400]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":1,"mask":0,"age":0,"heading":1500,"pos":[5000,0],"rp":[-512,400],"rh":1024,"angle":300,"off":[-512,0],"fb":0,"flag":3,"script":{"roll":[1,1,1,0,1,1],"face":[1,1,0,1,1,0],"arrive":[1,1,1,1,0,1],"permit":[1,1,0,0,1,1],"fwd":[0,1,1,1,0,0],"right":[0,0,1,1,0,1],"back":[1,1,1,1,1,1],"left":[1,1,0,1,1,0],"anim":[1,0,0,1,1,0],"turn":[0,1,1,1,0,1],"err":[0,0,1,1,1,0],"path":[1]},"log":[["arrive",-202,807],["turn",300],["anim"]],"ret":0,"state":0,"flag_out":1,"fb_out":0},
{"k":0,"mask":0,"age":9,"heading":1024,"pos":[256,256],"rp":[256,256],"rh":0,"angle":300,"off":[0,1000],"fb":0,"flag":1,"script":{"roll":[0,1,1,1,1,0],"face":[1,0,0,1,0,1],"arrive":[1,1,1,0,1,0],"permit":[0,1,0,1,0,1],"fwd":[0,1,1,1,0,0],"right":[1,1,1,1,0,1],"back":[1,0,1,1,1,0],"left":[1,1,0,1,1,1],"anim":[1,0,0,1,1,0],"turn":[1,1,0,0,0,0],"err":[1,1,1,1,1,0],"path":[1]},"log":[["arrive",-540,862],["turn",1324]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":3,"heading":0,"pos":[256,-2560],"rp":[-512,0],"rh":0,"angle":0,"off":[-512,0],"fb":0,"flag":3,"script":{"roll":[1,0,1,0,0,1],"face":[1,1,0,1,1,1],"arrive":[1,1,1,1,0,1],"permit":[0,1,1,1,0,1],"fwd":[1,0,1,1,0,1],"right":[1,0,1,0,0,1],"back":[1,1,1,0,0,1],"left":[1,1,1,0,1,0],"anim":[0,1,0,0,1,1],"turn":[1,1,1,1,1,1],"err":[1,1,0,1,1,1],"path":[1]},"log":[["arrive",-1024,0],["turn",1024]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":32,"age":0,"heading":1024,"pos":[256,-2560],"rp":[0,256],"rh":0,"angle":1024,"off":[0,1000],"fb":0,"flag":1,"script":{"roll":[0,0,1,0,0,0],"face":[0,1,0,0,1,1],"arrive":[1,1,0,1,0,0],"permit":[1,1,1,1,1,1],"fwd":[1,0,1,1,1,1],"right":[1,0,1,1,1,0],"back":[1,1,1,1,1,0],"left":[1,1,1,0,1,1],"anim":[0,0,1,1,1,1],"turn":[0,0,1,0,1,0],"err":[1,1,1,0,1,1],"path":[1]},"log":[["face",0,256]],"ret":0,"state":6,"flag_out":1,"fb_out":0},
{"k":1,"mask":8,"age":9,"heading":100,"pos":[256,256],"rp":[3000,-2560],"rh":1024,"angle":300,"off":[2048,256],"fb":0,"flag":3,"script":{"roll":[1,1,0,0,0,1],"face":[1,1,1,1,1,1],"arrive":[0,1,1,1,1,1],"permit":[1,0,0,1,0,1],"fwd":[0,1,1,1,1,1],"right":[1,0,1,0,1,1],"back":[1,1,1,1,1,1],"left":[0,0,1,0,1,1],"anim":[1,0,1,0,1,1],"turn":[1,0,0,0,1,0],"err":[0,1,1,1,0,1],"path":[1]},"log":[["arrive",1964,-4345],["turn",300]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":32,"age":3,"heading":1500,"pos":[-512,-2560],"rp":[0,400],"rh":1900,"angle":1024,"off":[-512,0],"fb":0,"flag":3,"script":{"roll":[0,1,1,1,1,1],"face":[0,1,0,1,0,0],"arrive":[1,1,0,1,1,1],"permit":[1,1,0,1,1,0],"fwd":[1,0,1,0,1,1],"right":[0,1,1,1,0,1],"back":[1,0,0,0,0,0],"left":[0,0,0,1,1,1],"anim":[1,1,1,1,1,1],"turn":[1,1,1,1,0,0],"err":[0,0,1,1,1,0],"path":[1]},"log":[["face",0,400]],"ret":0,"state":6,"flag_out":1,"fb_out":0},
{"k":1,"mask":256,"age":9,"heading":700,"pos":[0,-2560],"rp":[256,-2560],"rh":300,"angle":1900,"off":[2048,-2560],"fb":0,"flag":3,"script":{"roll":[1,1,0,1,1,1],"face":[1,1,0,1,0,1],"arrive":[1,1,1,0,1,0],"permit":[0,0,1,1,1,1],"fwd":[1,0,1,0,1,1],"right":[1,1,0,1,1,0],"back":[0,1,1,1,1,1],"left":[1,1,1,1,0,1],"anim":[1,0,1,1,0,1],"turn":[1,0,1,1,1,0],"err":[1,0,1,1,1,0],"path":[1]},"log":[["arrive",3236,-3926],["turn",1176]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":9,"heading":1500,"pos":[0,9000],"rp":[0,256],"rh":1900,"angle":2100,"off":[2048,-2560],"fb":0,"flag":1,"script":{"roll":[0,0,1,1,1,0],"face":[0,1,1,1,1,0],"arrive":[0,1,1,1,1,1],"permit":[1,1,1,0,1,0],"fwd":[1,1,1,0,1,1],"right":[1,1,1,0,1,0],"back":[0,1,1,1,0,0],"left":[1,1,1,1,1,1],"anim":[0,0,1,1,1,1],"turn":[1,1,0,1,0,1],"err":[1,0,1,1,1,1],"path":[1]},"log":[["arrive",1217,-2788],["face",1217,-2788],["permit",1217,-2788],["fwd",1217,-2788]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":3,"heading":2047,"pos":[256,9000],"rp":[-512,0],"rh":1900,"angle":300,"off":[256,1000],"fb":1,"flag":1,"script":{"roll":[1,1,1,0,0,1],"face":[1,1,1,0,1,1],"arrive":[1,1,1,0,0,1],"permit":[0,1,1,1,1,0],"fwd":[1,1,1,1,1,0],"right":[1,0,0,0,1,0],"back":[1,1,1,1,0,1],"left":[1,1,0,0,1,0],"anim":[1,0,0,0,1,0],"turn":[1,0,1,1,1,1],"err":[1,1,1,0,1,1],"path":[1]},"log":[["turn",1176]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":0,"mask":0,"age":9,"heading":2047,"pos":[5000,9000],"rp":[3000,256],"rh":1024,"angle":1024,"off":[256,1000],"fb":0,"flag":3,"script":{"roll":[1,1,0,1,1,0],"face":[1,0,1,1,0,1],"arrive":[1,1,1,1,1,0],"permit":[1,0,0,1,1,1],"fwd":[0,1,0,1,1,1],"right":[0,1,1,1,1,1],"back":[0,1,0,1,1,1],"left":[0,1,1,1,0,1],"anim":[1,1,1,1,0,1],"turn":[1,1,1,0,0,1],"err":[1,1,1,1,1,0],"path":[1]},"log":[["arrive",3256,1256],["turn",1024]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":3,"heading":1024,"pos":[-512,-2560],"rp":[256,256],"rh":300,"angle":300,"off":[0,-2560],"fb":0,"flag":1,"script":{"roll":[1,0,1,1,1,1],"face":[1,1,1,1,0,1],"arrive":[1,1,0,1,0,1],"permit":[0,1,0,1,1,1],"fwd":[0,0,1,1,1,0],"right":[1,1,1,1,0,1],"back":[0,1,1,1,1,1],"left":[1,0,1,0,1,0],"anim":[0,1,1,1,1,1],"turn":[1,0,1,1,0,0],"err":[1,0,0,1,1,0],"path":[1]},"log":[["arrive",2723,939],["turn",1624]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":0,"heading":1500,"pos":[-512,9000],"rp":[256,-2560],"rh":1024,"angle":2100,"off":[256,256],"fb":0,"flag":1,"script":{"roll":[1,1,0,1,0,1],"face":[1,0,1,1,1,1],"arrive":[1,0,1,0,1,1],"permit":[1,0,1,1,0,1],"fwd":[1,0,1,1,0,1],"right":[1,0,1,0,0,1],"back":[1,1,1,0,1,1],"left":[1,1,1,0,1,0],"anim":[1,1,1,0,1,1],"turn":[0,0,0,0,0,1],"err":[1,1,1,0,0,1],"path":[1]},"log":[["arrive",44,-2853],["turn",52],["anim"]],"ret":0,"state":0,"flag_out":1,"fb_out":0},
{"k":1,"mask":256,"age":0,"heading":100,"pos":[-512,9000],"rp":[0,-2560],"rh":0,"angle":1024,"off":[-512,1000],"fb":0,"flag":1,"script":{"roll":[0,1,0,1,1,0],"face":[1,1,1,0,1,1],"arrive":[1,1,0,1,0,1],"permit":[0,1,1,1,1,1],"fwd":[1,1,0,1,0,1],"right":[1,0,1,1,1,1],"back":[0,1,1,1,1,1],"left":[0,1,0,1,1,0],"anim":[0,0,1,1,0,1],"turn":[1,1,1,1,1,0],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",512,-3560],["turn",0]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":32,"age":9,"heading":2047,"pos":[-512,-2560],"rp":[-512,-2560],"rh":1900,"angle":300,"off":[256,-2560],"fb":0,"flag":3,"script":{"roll":[1,1,1,1,1,1],"face":[1,0,1,1,1,1],"arrive":[1,1,1,0,0,1],"permit":[1,0,1,1,1,1],"fwd":[0,1,1,0,0,1],"right":[1,0,1,1,1,1],"back":[1,1,1,0,1,0],"left":[1,1,1,1,0,1],"anim":[1,0,1,0,1,1],"turn":[1,1,1,1,1,1],"err":[1,1,0,1,1,0],"path":[1]},"log":[["face",-512,-2560]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":2,"age":3,"heading":0,"pos":[5000,-2560],"rp":[256,-2560],"rh":300,"angle":300,"off":[256,-2560],"fb":0,"flag":3,"script":{"roll":[0,1,1,1,0,1],"face":[1,1,0,1,1,1],"arrive":[0,1,0,1,1,0],"permit":[1,1,1,1,1,1],"fwd":[0,0,1,1,1,1],"right":[1,1,0,1,0,1],"back":[1,1,0,0,0,1],"left":[1,0,1,1,1,0],"anim":[1,1,1,1,0,1],"turn":[1,1,1,1,0,0],"err":[1,1,1,0,0,1],"path":[1]},"log":[["arrive",2655,-1630],["turn",1624]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":8,"age":0,"heading":700,"pos":[-512,0],"rp":[-512,0],"rh":1900,"angle":2100,"off":[256,-2560],"fb":1,"flag":3,"script":{"roll":[1,1,1,1,1,1],"face":[1,1,1,1,1,0],"arrive":[1,1,1,1,1,1],"permit":[1,1,1,0,1,0],"fwd":[1,1,1,0,1,1],"right":[1,0,0,1,1,0],"back":[0,1,1,0,1,0],"left":[0,1,1,1,1,1],"anim":[0,0,1,0,1,1],"turn":[0,1,1,1,1,0],"err":[1,1,1,1,0,1],"path":[1]},"log":[["face",-512,0]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":1,"mask":0,"age":0,"heading":0,"pos":[-512,9000],"rp":[256,0],"rh":300,"angle":1024,"off":[-512,1000],"fb":0,"flag":3,"script":{"roll":[1,1,0,0,1,0],"face":[0,1,0,1,1,0],"arrive":[1,1,1,1,1,1],"permit":[0,1,1,0,1,1],"fwd":[1,0,1,0,1,1],"right":[1,0,1,1,1,1],"back":[0,0,1,1,0,1],"left":[0,1,1,1,1,1],"anim":[0,1,1,1,1,1],"turn":[1,1,1,0,0,0],"err":[0,1,1,1,1,1],"path":[1]},"log":[["arrive",1362,-198],["turn",300]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":256,"age":3,"heading":1500,"pos":[256,-2560],"rp":[-512,400],"rh":300,"angle":300,"off":[0,256],"fb":1,"flag":1,"script":{"roll":[1,1,1,1,1,1],"face":[0,0,1,0,1,1],"arrive":[0,1,0,1,0,1],"permit":[0,1,0,0,1,0],"fwd":[1,0,1,1,1,1],"right":[1,1,1,1,1,1],"back":[1,0,0,0,0,1],"left":[0,1,1,0,1,1],"anim":[1,0,1,0,0,1],"turn":[1,0,0,1,1,1],"err":[1,1,1,0,1,0],"path":[1]},"log":[["face",-512,400]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":1,"mask":32,"age":0,"heading":700,"pos":[-512,9000],"rp":[256,256],"rh":300,"angle":1900,"off":[0,-2560],"fb":0,"flag":1,"script":{"roll":[0,0,0,1,1,1],"face":[1,1,0,1,1,1],"arrive":[1,1,1,0,0,1],"permit":[1,1,1,1,1,0],"fwd":[1,1,1,1,1,0],"right":[1,0,1,1,1,1],"back":[0,1,1,1,0,0],"left":[1,1,1,1,1,0],"anim":[1,1,1,1,1,0],"turn":[0,1,0,0,1,0],"err":[0,0,1,1,1,1],"path":[1]},"log":[["arrive",1407,-2031],["turn",1176],["anim"]],"ret":0,"state":0,"flag_out":1,"fb_out":0},
{"k":0,"mask":2,"age":0,"heading":1024,"pos":[5000,-2560],"rp":[-512,-2560],"rh":300,"angle":2100,"off":[0,0],"fb":0,"flag":3,"script":{"roll":[1,0,0,1,0,1],"face":[1,1,1,1,1,1],"arrive":[0,1,1,1,0,1],"permit":[0,0,1,1,1,1],"fwd":[1,0,1,1,1,0],"right":[1,1,1,1,1,0],"back":[0,1,0,1,0,0],"left":[1,0,1,1,1,1],"anim":[1,1,1,0,1,1],"turn":[1,0,1,1,1,1],"err":[1,1,1,0,1,0],"path":[1]},"log":[["face",-512,-2560]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":9,"heading":0,"pos":[-512,0],"rp":[256,256],"rh":300,"angle":0,"off":[2048,-2560],"fb":0,"flag":3,"script":{"roll":[1,0,1,0,1,0],"face":[0,1,1,0,1,1],"arrive":[0,1,1,1,0,1],"permit":[1,1,0,1,1,1],"fwd":[1,1,0,0,1,1],"right":[1,1,1,1,0,0],"back":[1,0,0,0,1,1],"left":[0,0,1,1,1,1],"anim":[0,1,1,0,0,1],"turn":[0,0,1,1,1,0],"err":[1,1,1,0,0,1],"path":[1]},"log":[["arrive",3533,336],["turn",1324],["permit",3533,336],["right",3533,336]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":9,"heading":1024,"pos":[-512,256],"rp":[-512,400],"rh":300,"angle":1024,"off":[0,-2560],"fb":1,"flag":3,"script":{"roll":[1,1,1,0,1,1],"face":[1,1,1,1,0,1],"arrive":[0,0,0,1,1,1],"permit":[1,0,1,1,0,1],"fwd":[1,1,0,0,1,1],"right":[1,1,1,1,0,1],"back":[1,0,0,1,0,0],"left":[1,1,0,0,1,1],"anim":[1,0,1,0,1,1],"turn":[1,1,1,0,0,1],"err":[1,0,1,1,1,0],"path":[1]},"log":[["face",-512,400]],"ret":1,"state":0,"flag_out":3,"fb_out":1},
{"k":1,"mask":0,"age":9,"heading":700,"pos":[5000,0],"rp":[-512,-2560],"rh":300,"angle":2100,"off":[0,256],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,0,1],"face":[0,1,0,1,1,1],"arrive":[1,1,0,0,1,1],"permit":[1,1,1,1,1,0],"fwd":[1,1,1,1,1,1],"right":[1,1,1,0,0,1],"back":[1,0,0,1,0,1],"left":[0,1,0,1,1,1],"anim":[1,1,0,0,0,1],"turn":[0,0,0,1,1,1],"err":[1,1,0,0,0,1],"path":[1]},"log":[["arrive",-738,-2439],["turn",1376],["anim"]],"ret":0,"state":0,"flag_out":1,"fb_out":0},
{"k":0,"mask":2,"age":0,"heading":100,"pos":[5000,-2560],"rp":[0,0],"rh":1024,"angle":0,"off":[256,256],"fb":0,"flag":3,"script":{"roll":[1,0,1,1,0,0],"face":[1,1,1,0,1,1],"arrive":[1,1,1,1,1,0],"permit":[1,1,0,1,1,1],"fwd":[1,1,1,0,1,1],"right":[0,1,1,1,1,0],"back":[0,1,1,0,1,0],"left":[1,1,1,1,0,1],"anim":[1,1,1,0,1,1],"turn":[1,0,1,1,0,0],"err":[1,1,0,0,1,0],"path":[1]},"log":[["face",0,0]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":256,"age":9,"heading":2047,"pos":[5000,-2560],"rp":[256,0],"rh":1024,"angle":0,"off":[0,-2560],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,0,0],"face":[0,1,1,0,1,0],"arrive":[0,1,0,1,1,1],"permit":[1,1,1,0,1,1],"fwd":[1,1,1,1,1,1],"right":[1,1,0,0,1,1],"back":[0,1,1,1,1,1],"left":[0,1,0,0,1,1],"anim":[1,0,1,0,1,1],"turn":[1,1,1,1,1,0],"err":[1,1,0,1,1,0],"path":[1]},"log":[["arrive",256,2560],["turn",0]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":8,"age":0,"heading":700,"pos":[0,9000],"rp":[256,400],"rh":1024,"angle":2100,"off":[-512,-2560],"fb":0,"flag":3,"script":{"roll":[1,0,0,1,1,1],"face":[1,1,0,1,1,0],"arrive":[1,1,1,1,1,0],"permit":[1,1,0,1,1,0],"fwd":[1,0,1,0,1,1],"right":[1,0,1,0,0,1],"back":[1,1,1,1,0,0],"left":[1,1,0,0,0,1],"anim":[0,0,1,0,1,1],"turn":[0,1,1,1,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",355,3009],["turn",52],["anim"]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":2,"age":0,"heading":100,"pos":[-512,256],"rp":[3000,0],"rh":300,"angle":1900,"off":[-512,1000],"fb":0,"flag":3,"script":{"roll":[0,0,1,1,1,0],"face":[1,0,0,0,1,0],"arrive":[1,1,0,0,1,0],"permit":[0,1,1,1,1,0],"fwd":[1,1,1,0,1,1],"right":[1,1,1,0,1,1],"back":[0,1,1,1,0,0],"left":[0,1,0,1,0,0],"anim":[0,0,1,0,1,1],"turn":[0,1,0,0,0,1],"err":[0,1,1,0,1,1],"path":[1]},"log":[["arrive",2093,663],["turn",1176],["anim"]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":9,"heading":100,"pos":[256,-2560],"rp":[256,-2560],"rh":1024,"angle":2100,"off":[2048,-2560],"fb":1,"flag":3,"script":{"roll":[0,0,0,0,1,0],"face":[0,1,1,0,0,0],"arrive":[1,1,0,1,1,0],"permit":[1,1,1,1,1,1],"fwd":[0,1,0,1,0,1],"right":[1,1,1,1,1,1],"back":[1,1,0,1,1,1],"left":[1,0,1,1,1,1],"anim":[1,1,1,1,0,0],"turn":[1,0,1,1,1,1],"err":[0,1,1,0,1,1],"path":[1]},"log":[["face",256,-2560]],"ret":0,"state":6,"flag_out":1,"fb_out":1},
{"k":1,"mask":0,"age":3,"heading":1500,"pos":[256,256],"rp":[-512,0],"rh":300,"angle":1900,"off":[0,256],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,1,1],"face":[1,1,1,1,0,1],"arrive":[1,1,1,1,1,1],"permit":[1,0,0,1,0,1],"fwd":[0,1,0,0,1,1],"right":[1,0,1,0,0,1],"back":[0,1,1,1,1,1],"left":[1,1,1,0,1,0],"anim":[1,0,1,1,0,0],"turn":[1,1,1,1,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",-627,229],["turn",1176]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":0,"heading":700,"pos":[0,9000],"rp":[-512,256],"rh":0,"angle":1900,"off":[2048,1000],"fb":0,"flag":1,"script":{"roll":[1,0,0,0,1,1],"face":[1,1,1,1,1,0],"arrive":[1,1,1,1,1,1],"permit":[1,1,0,0,0,0],"fwd":[1,1,1,0,1,0],"right":[1,1,0,1,1,1],"back":[0,1,0,1,1,0],"left":[1,1,1,1,1,1],"anim":[1,1,1,1,1,0],"turn":[1,0,0,1,1,0],"err":[0,0,0,1,1,1],"path":[1]},"log":[["arrive",1767,256],["turn",876]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":32,"age":3,"heading":0,"pos":[5000,0],"rp":[3000,0],"rh":0,"angle":2100,"off":[256,1000],"fb":0,"flag":1,"script":{"roll":[1,1,0,1,0,1],"face":[1,0,1,1,0,0],"arrive":[1,0,1,1,0,0],"permit":[1,1,0,1,0,0],"fwd":[1,1,1,1,1,1],"right":[0,0,1,0,1,0],"back":[0,0,0,1,0,1],"left":[1,0,1,0,1,1],"anim":[0,1,1,1,1,1],"turn":[1,0,1,0,1,1],"err":[1,0,0,0,1,1],"path":[1]},"log":[["arrive",3094,1028],["turn",1076]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":9,"heading":700,"pos":[-512,0],"rp":[0,-2560],"rh":1024,"angle":1024,"off":[-512,0],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,0,0],"face":[1,1,0,0,1,1],"arrive":[1,1,1,1,0,0],"permit":[1,1,1,0,1,1],"fwd":[0,0,0,1,1,1],"right":[1,0,0,1,0,1],"back":[1,1,1,0,1,0],"left":[1,0,0,1,1,0],"anim":[0,1,0,0,1,1],"turn":[1,1,1,1,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",-512,-2560],["turn",1024]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":0,"age":0,"heading":0,"pos":[0,9000],"rp":[-512,256],"rh":1024,"angle":300,"off":[256,1000],"fb":0,"flag":3,"script":{"roll":[0,0,1,0,0,1],"face":[1,0,0,0,1,1],"arrive":[1,1,0,0,1,1],"permit":[1,1,1,1,0,1],"fwd":[0,0,1,0,1,1],"right":[0,1,0,1,0,1],"back":[1,0,1,1,1,1],"left":[0,1,0,0,1,1],"anim":[0,0,1,0,1,1],"turn":[0,0,0,1,0,1],"err":[1,1,0,1,1,1],"path":[1]},"log":[["arrive",129,-553],["turn",300],["anim"]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":8,"age":0,"heading":2047,"pos":[256,256],"rp":[3000,-2560],"rh":0,"angle":1024,"off":[2048,0],"fb":0,"flag":3,"script":{"roll":[0,1,0,1,0,1],"face":[1,0,1,0,1,0],"arrive":[1,0,0,1,1,0],"permit":[1,1,1,1,1,0],"fwd":[1,1,1,0,1,0],"right":[0,1,1,0,0,0],"back":[0,1,1,0,0,0],"left":[1,1,1,1,0,1],"anim":[1,1,1,1,1,1],"turn":[0,0,0,1,0,1],"err":[1,0,1,0,1,0],"path":[1]},"log":[["arrive",952,-2560],["turn",0],["anim"]],"ret":0,"state":0,"flag_out":1,"fb_out":0},
{"k":1,"mask":2,"age":3,"heading":700,"pos":[-512,256],"rp":[0,0],"rh":300,"angle":0,"off":[-512,-2560],"fb":0,"flag":1,"script":{"roll":[1,1,0,1,1,0],"face":[0,0,0,0,1,0],"arrive":[1,1,1,1,1,1],"permit":[1,0,1,1,1,1],"fwd":[1,1,0,0,1,1],"right":[1,0,1,0,1,1],"back":[0,0,0,1,1,1],"left":[1,1,0,1,0,1],"anim":[0,1,1,1,0,0],"turn":[0,0,1,0,1,1],"err":[0,0,1,0,1,1],"path":[1]},"log":[["arrive",1727,-1958],["turn",1324],["anim"]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":8,"age":3,"heading":2047,"pos":[5000,9000],"rp":[-512,-2560],"rh":0,"angle":300,"off":[256,1000],"fb":0,"flag":3,"script":{"roll":[1,1,0,1,1,0],"face":[0,1,1,0,1,1],"arrive":[0,1,1,1,0,1],"permit":[0,0,1,1,1,1],"fwd":[0,0,0,0,0,0],"right":[0,1,1,0,0,1],"back":[0,0,1,0,1,1],"left":[1,0,1,0,1,1],"anim":[0,1,1,0,1,0],"turn":[1,0,0,1,0,1],"err":[0,1,0,1,0,1],"path":[1]},"log":[["arrive",-1153,-1751],["face",-1153,-1751],["permit",-1153,-1751]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":2,"age":0,"heading":1500,"pos":[5000,0],"rp":[3000,256],"rh":1900,"angle":300,"off":[2048,256],"fb":0,"flag":1,"script":{"roll":[1,1,1,0,1,1],"face":[1,1,1,0,1,0],"arrive":[1,0,0,1,1,1],"permit":[1,0,0,1,1,0],"fwd":[1,0,1,0,1,1],"right":[1,0,1,0,0,1],"back":[1,0,1,1,1,1],"left":[1,1,1,1,1,0],"anim":[1,1,1,1,0,1],"turn":[1,1,0,1,1,1],"err":[0,1,0,1,0,1],"path":[1]},"log":[["arrive",4714,1405],["turn",1176]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":0,"age":0,"heading":0,"pos":[256,256],"rp":[3000,0],"rh":0,"angle":0,"off":[2048,256],"fb":1,"flag":1,"script":{"roll":[1,1,1,0,0,1],"face":[1,0,1,1,1,1],"arrive":[1,1,1,1,1,0],"permit":[0,1,0,0,1,0],"fwd":[0,1,1,1,0,1],"right":[1,1,0,1,1,1],"back":[1,1,1,0,1,1],"left":[1,1,1,1,1,1],"anim":[0,1,1,1,0,1],"turn":[0,1,1,1,1,1],"err":[1,0,1,0,0,1],"path":[1]},"log":[["face",3000,0]],"ret":1,"state":0,"flag_out":1,"fb_out":1},
{"k":1,"mask":256,"age":9,"heading":100,"pos":[-512,0],"rp":[3000,-2560],"rh":0,"angle":2100,"off":[256,-2560],"fb":0,"flag":1,"script":{"roll":[1,0,1,1,1,1],"face":[0,1,0,1,1,1],"arrive":[0,0,0,1,1,1],"permit":[1,1,1,1,1,0],"fwd":[0,0,1,1,0,0],"right":[1,1,1,0,1,1],"back":[0,0,1,1,1,1],"left":[0,1,1,0,1,1],"anim":[0,0,1,1,1,1],"turn":[1,1,1,1,1,1],"err":[0,0,0,0,0,1],"path":[1]},"log":[["arrive",3659,-5047],["turn",1076]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":32,"age":0,"heading":2047,"pos":[5000,-2560],"rp":[256,0],"rh":300,"angle":300,"off":[2048,-2560],"fb":0,"flag":1,"script":{"roll":[1,1,1,1,1,1],"face":[1,1,1,1,0,1],"arrive":[1,1,1,1,1,0],"permit":[1,0,0,0,1,1],"fwd":[1,1,0,1,1,1],"right":[1,1,1,1,1,0],"back":[1,0,0,1,1,0],"left":[1,1,1,1,1,1],"anim":[0,0,1,1,1,0],"turn":[0,1,0,1,1,0],"err":[1,1,1,1,0,0],"path":[1]},"log":[["face",256,0]],"ret":1,"state":0,"flag_out":1,"fb_out":0},
{"k":1,"mask":32,"age":3,"heading":0,"pos":[5000,256],"rp":[3000,0],"rh":0,"angle":300,"off":[-512,1000],"fb":0,"flag":1,"script":{"roll":[1,0,1,0,1,1],"face":[1,1,1,1,1,1],"arrive":[1,0,0,0,1,0],"permit":[0,0,1,1,1,1],"fwd":[1,1,0,1,1,1],"right":[1,1,1,0,1,1],"back":[0,1,0,1,1,1],"left":[1,1,1,0,0,1],"anim":[0,1,0,1,0,1],"turn":[0,1,1,1,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",1894,198],["turn",1324],["anim"]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":0,"mask":8,"age":3,"heading":700,"pos":[256,9000],"rp":[-512,0],"rh":1900,"angle":1900,"off":[0,1000],"fb":0,"flag":1,"script":{"roll":[1,1,0,1,0,0],"face":[1,1,1,0,0,0],"arrive":[0,1,0,1,1,1],"permit":[0,1,1,1,1,1],"fwd":[1,1,0,0,1,1],"right":[1,0,1,1,0,1],"back":[1,0,0,1,1,1],"left":[1,1,0,1,0,1],"anim":[1,0,1,1,1,0],"turn":[1,1,1,0,0,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",276,615],["face",276,615]],"ret":1,"state":0,"flag_out":3,"fb_out":0},
{"k":1,"mask":32,"age":0,"heading":700,"pos":[0,9000],"rp":[0,400],"rh":300,"angle":2100,"off":[0,0],"fb":0,"flag":3,"script":{"roll":[1,1,0,1,0,0],"face":[1,0,1,1,1,1],"arrive":[0,0,1,0,0,1],"permit":[0,1,1,1,1,1],"fwd":[1,1,1,0,1,1],"right":[1,0,0,0,1,0],"back":[1,0,0,1,1,1],"left":[1,1,1,1,1,1],"anim":[1,1,1,1,1,1],"turn":[1,0,0,0,1,1],"err":[1,1,1,1,1,1],"path":[1]},"log":[["arrive",0,400],["turn",1376]],"ret":1,"state":0,"flag_out":3,"fb_out":0}
]"#,
        )
        .unwrap();
        for (index, row) in rows.into_iter().enumerate() {
            let mut port = Scripted {
                stale: row.mask != 0 && row.age > 8,
                heading: row.heading,
                position: row.pos,
                script: row.script,
                used: BTreeMap::new(),
                log: Vec::new(),
                called: 7,
                state: 0,
                hook: row.flag & 2 != 0,
                masked: row.mask & 0x1e2 != 0,
                age_over: row.age > 8,
            };
            let reference = Reference {
                position: [row.rp[0], 0, row.rp[1]],
                heading: row.rh,
            };
            let mut state = AlignState {
                fallback: row.fb != 0,
            };
            let result = if row.k == 0 {
                approach_mover(&mut port, &mut state, row.angle, row.off, reference)
            } else {
                close_mover(&mut port, &mut state, row.angle, row.off, reference)
            }
            .unwrap();
            let expected: Vec<(String, Vec<i32>)> = row
                .log
                .into_iter()
                .map(|entry| {
                    (
                        entry[0].as_str().unwrap().to_owned(),
                        entry[1..]
                            .iter()
                            .map(|value| value.as_i64().unwrap() as i32)
                            .collect(),
                    )
                })
                .collect();
            let label = format!("row {index} mover {}", row.k);
            assert_eq!(port.log, expected, "{label}");
            assert_eq!(result, row.ret, "{label}");
            assert_eq!(port.state, row.state, "{label}");
            assert_eq!(u8::from(port.hook) * 2, row.flag_out & 2, "{label}");
            assert_eq!(u32::from(state.fallback), row.fb_out, "{label}");
        }
    }

    struct Mover {
        position: [i32; 3],
        heading: u16,
        counter: u8,
        animation: u8,
        delta: [i32; 2],
        called: [bool; 4],
    }

    impl StepPort for Mover {
        type Saved = ([i32; 3], u8, u8);
        fn position(&self) -> [i32; 3] {
            self.position
        }
        fn heading(&self) -> u16 {
            self.heading
        }
        fn save(&mut self) -> Self::Saved {
            (self.position, self.counter, self.animation)
        }
        fn restore(&mut self, saved: Self::Saved) {
            (self.position, self.counter, self.animation) = saved;
        }
        fn walk(&mut self, _: CloseStep) {
            self.position[0] += self.delta[0];
            self.position[2] += self.delta[1];
            self.counter = self.counter.wrapping_add(0x15);
            self.animation = 0xaa;
            self.called = [true; 4];
        }
        fn clear_called(&mut self, step: CloseStep) {
            self.called[match step {
                CloseStep::Left => 0,
                CloseStep::Right => 1,
                CloseStep::Forward => 2,
                CloseStep::Backward => 3,
            }] = false;
        }
        fn set_horizontal(&mut self, x: i32, z: i32) {
            self.position[0] = x;
            self.position[2] = z;
        }
    }

    #[test]
    fn the_close_steps_match_the_original_helpers_with_a_scripted_walk() {
        // Rows from the original helpers (left, right, backward, forward) in an emulator with the
        // one-unit walk replaced by a fixed displacement: helper, heading, start, target, walk
        // displacement, kept, final position, final step counter, start step counter, the step's
        // called flag afterwards.
        type Row = (
            u8,
            u16,
            [i32; 3],
            [i32; 2],
            [i32; 2],
            u8,
            [i32; 3],
            u8,
            u8,
            u8,
        );
        #[rustfmt::skip]
        let rows: &[Row] = &[
            (0, 1700, [256, 777, 9000], [3000, 0], [-2048, 2048], 0, [3000, 777, 0], 95, 95, 0),
            (3, 1700, [256, 777, 0], [0, -7000], [512, 0], 0, [0, 777, -7000], 202, 202, 0),
            (0, 300, [-2560, 777, 0], [0, 0], [0, 128], 0, [0, 777, 0], 123, 123, 0),
            (3, 1700, [256, 777, 512], [3000, -7000], [128, -128], 0, [3000, 777, -7000], 255, 255, 0),
            (3, 0, [256, 777, 9000], [-5000, 400], [-2048, 0], 0, [-5000, 777, 400], 130, 130, 0),
            (2, 300, [-2560, 777, 512], [0, 0], [-2048, 0], 0, [0, 777, 0], 205, 205, 0),
            (3, 1024, [256, 777, 0], [0, -7000], [0, 128], 1, [256, 777, 128], 128, 107, 1),
            (3, 1700, [256, 777, 9000], [256, 400], [0, -2048], 0, [256, 777, 400], 101, 101, 0),
            (2, 1024, [0, 777, 512], [-5000, 0], [2048, 0], 0, [-5000, 777, 0], 68, 68, 0),
            (1, 0, [0, 777, 0], [256, 400], [128, -2048], 0, [256, 777, 400], 96, 96, 0),
            (0, 300, [-2560, 777, 0], [256, -7000], [2048, 0], 1, [-512, 777, 0], 223, 202, 1),
            (0, 300, [0, 777, 512], [-5000, 0], [128, 128], 0, [-5000, 777, 0], 201, 201, 0),
            (3, 0, [0, 777, 0], [256, 400], [0, -2048], 1, [0, 777, -2048], 189, 168, 1),
            (2, 1700, [0, 777, 0], [0, 0], [-2048, 128], 0, [0, 777, 0], 7, 7, 0),
            (2, 1024, [-2560, 777, 512], [3000, -7000], [2048, -2048], 1, [-512, 777, -1536], 90, 69, 1),
            (0, 300, [-2560, 777, 0], [-5000, 0], [-2048, 128], 1, [-4608, 777, 128], 118, 97, 1),
            (1, 300, [-2560, 777, 512], [256, -7000], [0, 2048], 0, [256, 777, -7000], 24, 24, 0),
            (3, 0, [0, 777, 9000], [-5000, 0], [512, 2048], 0, [-5000, 777, 0], 131, 131, 0),
            (0, 1700, [256, 777, 9000], [3000, -7000], [0, 128], 0, [3000, 777, -7000], 116, 116, 0),
            (0, 0, [256, 777, 0], [3000, -7000], [0, 0], 0, [3000, 777, -7000], 137, 137, 0),
            (0, 1700, [0, 777, 0], [0, 0], [-128, -128], 0, [0, 777, 0], 67, 67, 0),
            (3, 1024, [0, 777, 512], [-5000, -7000], [512, -2048], 0, [-5000, 777, -7000], 71, 71, 0),
            (3, 0, [256, 777, 512], [-5000, 0], [0, -2048], 0, [-5000, 777, 0], 38, 38, 0),
            (0, 0, [-2560, 777, 512], [-5000, 0], [0, 0], 1, [-2560, 777, 512], 252, 231, 1),
            (2, 0, [-2560, 777, 9000], [3000, 400], [-128, 0], 0, [3000, 777, 400], 242, 242, 0),
            (3, 1700, [0, 777, 512], [0, -7000], [512, 2048], 0, [0, 777, -7000], 194, 194, 0),
            (3, 0, [0, 777, 0], [0, 400], [2048, -128], 0, [0, 777, 400], 198, 198, 0),
            (0, 1700, [256, 777, 9000], [0, -7000], [-2048, 0], 0, [0, 777, -7000], 158, 158, 0),
            (3, 1700, [0, 777, 0], [0, 400], [-2048, 2048], 0, [0, 777, 400], 130, 130, 0),
            (3, 1024, [256, 777, 0], [3000, -7000], [128, -128], 1, [384, 777, -128], 247, 226, 1),
            (0, 300, [256, 777, 512], [-5000, 0], [512, 2048], 1, [768, 777, 2560], 123, 102, 1),
            (1, 1700, [0, 777, 9000], [-5000, 0], [128, 128], 0, [-5000, 777, 0], 254, 254, 0),
            (2, 0, [-2560, 777, 0], [-5000, 0], [0, 2048], 0, [-5000, 777, 0], 241, 241, 0),
            (2, 300, [256, 777, 512], [256, -7000], [-128, -2048], 1, [128, 777, -1536], 252, 231, 1),
            (2, 0, [0, 777, 512], [0, -7000], [512, -128], 1, [512, 777, 384], 202, 181, 1),
            (2, 0, [-2560, 777, 0], [256, 400], [128, 0], 1, [-2432, 777, 0], 157, 136, 1),
            (1, 300, [0, 777, 9000], [0, 400], [0, -2048], 0, [0, 777, 400], 188, 188, 0),
            (3, 300, [0, 777, 512], [-5000, 0], [512, 0], 1, [512, 777, 512], 6, 241, 1),
            (3, 300, [0, 777, 512], [-5000, -7000], [512, -2048], 0, [-5000, 777, -7000], 81, 81, 0),
            (0, 300, [0, 777, 512], [256, 0], [2048, 2048], 1, [2048, 777, 2560], 121, 100, 1),
            (3, 1700, [-2560, 777, 9000], [-5000, 400], [-128, 128], 0, [-5000, 777, 400], 50, 50, 0),
            (2, 1700, [0, 777, 0], [0, 400], [0, 128], 1, [0, 777, 128], 192, 171, 1),
            (1, 1700, [0, 777, 9000], [256, 400], [-2048, 0], 1, [-2048, 777, 9000], 134, 113, 1),
            (0, 1024, [-2560, 777, 9000], [3000, -7000], [-2048, -2048], 0, [3000, 777, -7000], 131, 131, 0),
            (2, 1700, [-2560, 777, 0], [-5000, 0], [-2048, -2048], 1, [-4608, 777, -2048], 160, 139, 1),
            (0, 300, [256, 777, 9000], [0, -7000], [0, -2048], 1, [256, 777, 6952], 214, 193, 1),
            (0, 1700, [256, 777, 512], [3000, 400], [2048, 128], 0, [3000, 777, 400], 173, 173, 0),
            (0, 1700, [-2560, 777, 512], [0, 0], [2048, -2048], 0, [0, 777, 0], 15, 15, 0),
            (2, 300, [-2560, 777, 0], [0, 0], [128, 128], 1, [-2432, 777, 128], 27, 6, 1),
            (2, 1024, [-2560, 777, 512], [3000, -7000], [2048, 0], 1, [-512, 777, 512], 20, 255, 1),
            (3, 1024, [-2560, 777, 0], [256, 0], [2048, 2048], 1, [-512, 777, 2048], 102, 81, 1),
            (1, 300, [-2560, 777, 0], [3000, 0], [512, -128], 0, [3000, 777, 0], 92, 92, 0),
            (2, 1024, [0, 777, 0], [3000, 0], [128, 128], 1, [128, 777, 128], 88, 67, 1),
            (0, 0, [0, 777, 0], [-5000, 400], [0, -2048], 1, [0, 777, -2048], 196, 175, 1),
            (1, 0, [256, 777, 0], [0, 0], [-2048, -2048], 0, [0, 777, 0], 246, 246, 0),
            (1, 300, [0, 777, 0], [3000, 0], [128, -128], 0, [3000, 777, 0], 48, 48, 0),
            (0, 1024, [256, 777, 0], [-5000, 0], [512, 0], 0, [-5000, 777, 0], 102, 102, 0),
            (1, 1700, [256, 777, 512], [-5000, 400], [128, -2048], 0, [-5000, 777, 400], 84, 84, 0),
            (2, 1024, [0, 777, 9000], [3000, 400], [128, -128], 1, [128, 777, 8872], 210, 189, 1),
            (0, 300, [256, 777, 0], [3000, 0], [-128, -128], 0, [3000, 777, 0], 240, 240, 0),
            (1, 1700, [-2560, 777, 0], [0, 400], [-2048, 2048], 1, [-4608, 777, 2048], 130, 109, 1),
            (2, 1024, [256, 777, 512], [256, -7000], [-2048, 0], 0, [256, 777, -7000], 227, 227, 0),
            (1, 1700, [-2560, 777, 512], [0, 400], [512, 2048], 1, [-2048, 777, 2560], 46, 25, 1),
            (2, 0, [0, 777, 512], [256, 400], [128, 0], 1, [128, 777, 512], 71, 50, 1),
            (2, 300, [0, 777, 512], [0, -7000], [-128, -2048], 1, [-128, 777, -1536], 204, 183, 1),
            (2, 1024, [-2560, 777, 9000], [3000, -7000], [-128, 128], 0, [3000, 777, -7000], 97, 97, 0),
            (1, 300, [-2560, 777, 0], [0, 0], [-128, -128], 0, [0, 777, 0], 217, 217, 0),
            (2, 1700, [256, 777, 512], [3000, 400], [-128, 2048], 0, [3000, 777, 400], 68, 68, 0),
            (1, 1024, [0, 777, 9000], [-5000, 400], [-2048, 128], 0, [-5000, 777, 400], 218, 218, 0),
            (2, 0, [256, 777, 512], [0, -7000], [0, 2048], 0, [0, 777, -7000], 248, 248, 0),
            (2, 1700, [-2560, 777, 9000], [-5000, 400], [128, 128], 0, [-5000, 777, 400], 183, 183, 0),
            (2, 1024, [0, 777, 512], [256, -7000], [-2048, -128], 0, [256, 777, -7000], 224, 224, 0),
            (2, 1700, [256, 777, 0], [256, -7000], [-2048, 128], 0, [256, 777, -7000], 43, 43, 0),
            (1, 1700, [-2560, 777, 512], [256, 400], [-128, -128], 0, [256, 777, 400], 170, 170, 0),
            (1, 1700, [256, 777, 9000], [0, -7000], [-2048, 0], 1, [-1792, 777, 9000], 231, 210, 1),
            (1, 1024, [0, 777, 512], [256, -7000], [-128, -128], 0, [256, 777, -7000], 30, 30, 0),
            (3, 1700, [-2560, 777, 512], [3000, 400], [-128, -128], 1, [-2688, 777, 384], 228, 207, 1),
            (3, 1024, [256, 777, 9000], [0, 400], [0, 128], 1, [256, 777, 9128], 75, 54, 1),
            (3, 1024, [256, 777, 512], [256, -7000], [512, -128], 1, [768, 777, 384], 249, 228, 1),
            (0, 0, [0, 777, 0], [0, 0], [512, 128], 1, [512, 777, 128], 113, 92, 1),
            (1, 1700, [0, 777, 512], [0, 0], [128, 0], 1, [128, 777, 512], 101, 80, 1),
            (1, 1700, [256, 777, 512], [256, -7000], [0, 2048], 1, [256, 777, 2560], 183, 162, 1),
            (1, 0, [256, 777, 512], [3000, 400], [-128, 2048], 1, [128, 777, 2560], 75, 54, 1),
            (0, 300, [-2560, 777, 512], [-5000, 0], [-2048, 128], 1, [-4608, 777, 640], 150, 129, 1),
            (3, 1024, [0, 777, 9000], [-5000, -7000], [128, 0], 1, [128, 777, 9000], 22, 1, 1),
            (3, 1024, [0, 777, 512], [256, -7000], [2048, -2048], 1, [2048, 777, -1536], 185, 164, 1),
            (0, 300, [256, 777, 9000], [256, 400], [0, 128], 1, [256, 777, 9128], 135, 114, 1),
            (3, 1024, [256, 777, 512], [-5000, -7000], [128, 0], 1, [384, 777, 512], 86, 65, 1),
            (0, 300, [0, 777, 9000], [256, -7000], [512, 0], 1, [512, 777, 9000], 22, 1, 1),
            (0, 1700, [0, 777, 0], [-5000, 400], [-128, -2048], 1, [-128, 777, -2048], 132, 111, 1),
            (1, 1700, [-2560, 777, 512], [-5000, -7000], [0, 2048], 1, [-2560, 777, 2560], 82, 61, 1),
            (3, 1024, [0, 777, 9000], [-5000, 0], [512, 2048], 1, [512, 777, 11048], 225, 204, 1),
            (1, 1700, [256, 777, 9000], [3000, 400], [0, 2048], 1, [256, 777, 11048], 185, 164, 1),
            (1, 1700, [-2560, 777, 512], [0, 0], [2048, 0], 1, [-512, 777, 512], 140, 119, 1),
            (1, 1700, [256, 777, 9000], [3000, 0], [0, -2048], 1, [256, 777, 6952], 246, 225, 1),
            (1, 1700, [0, 777, 9000], [256, 400], [0, 0], 1, [0, 777, 9000], 126, 105, 1),
        ];
        let steps = [
            CloseStep::Left,
            CloseStep::Right,
            CloseStep::Backward,
            CloseStep::Forward,
        ];
        for (index, row) in rows.iter().enumerate() {
            let &(
                helper,
                heading,
                position,
                target,
                delta,
                kept,
                after,
                counter_after,
                counter,
                flag,
            ) = row;
            let mut mover = Mover {
                position,
                heading,
                counter,
                animation: 0,
                delta,
                called: [false; 4],
            };
            let result = close_step_run(&mut mover, steps[helper as usize], target[0], target[1]);
            assert_eq!(u8::from(result), kept, "row {index}");
            assert_eq!(mover.position, after, "row {index}");
            assert_eq!(mover.counter, counter_after, "row {index}");
            assert_eq!(mover.animation == 0, kept == 0, "row {index}");
            assert_eq!(
                u8::from(mover.called[[0, 1, 3, 2][helper as usize]]),
                flag,
                "row {index}"
            );
        }
    }
}
