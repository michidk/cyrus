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
}

/// The player state a directional step helper moves and, when it rejects the step, restores.
pub trait StepPort {
    /// Position, step counter and animation state captured before the step.
    type Saved;
    fn position(&self) -> [i32; 3];
    fn heading(&self) -> u16;
    fn save(&self) -> Self::Saved;
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

/// The pickup mover's retained state: the fallback flag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PickupState {
    pub fallback: bool,
}

/// Completion: the default animation request, which reports completion.
fn finished<P: LineupPort>(port: &mut P) -> i32 {
    i32::from(!port.default_animation())
}

fn stale_exit<P: LineupPort>(port: &mut P) -> i32 {
    if port.stale_response() {
        port.abandon(true);
        0
    } else {
        1
    }
}

/// `PlayerPickupLineUp`'s mover: zero when done, one while continuing. On the first invocation
/// (elapsed zero) a blocked straight path selects the fallback, the plain mover, for good.
pub fn pickup_mover<P: LineupPort>(
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
pub fn plain_mover<P: LineupPort>(port: &mut P, angle: i32, target: [i32; 3]) -> i32 {
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
        fn save(&self) -> Self::Saved {
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
