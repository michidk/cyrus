//! The follow camera's pose for one player pose observed in the original game.
//!
//! **Verified** against the original: in the START world the original's debug overlay showed the
//! player at (32464, -275, 33892) with angles (0, 22, 0) and the camera at (32481, -330, 33642)
//! with angles (0, 2027, 0), whole map units, while the installed `[camera]` settings were the
//! defaults below (offset (0, -14000, 0), distance 250, target offset (-6000, -14000, 0)).
use cyrus_game::script::{ScriptActorId, camera::*};

#[test]
fn follow_camera_matches_the_original_overlay_for_one_player_pose() {
    let actor = ScriptActorId {
        generation: 1,
        index: 0,
    };
    let settings = CameraSettings {
        offset_pos: [0, -14000, 0],
        offset_angle: [0, 0, 0],
        target_offset: [-6000, -14000, 0],
        distance: 250,
        min_distance: 120,
        max_vel: 100_000,
        ..Default::default()
    };
    let mut state = CameraState::new(&settings, actor, actor, 0).unwrap();
    let player = [32464 << 8, -275 << 8, 33892 << 8];
    let angles = [0, 22, 0];
    let candidate = state.records[0].candidate(player, angles).unwrap();
    let pose = state.orient(candidate, player, angles).unwrap();
    let map = pose.position.map(|axis| axis >> 8);
    for (axis, (ours, theirs)) in map.into_iter().zip([32481, -330, 33642]).enumerate() {
        assert!(
            (ours - theirs).abs() <= 1,
            "position axis {axis}: {ours} against the original's {theirs}"
        );
    }
    assert_eq!(pose.angles[0], 0, "pitch");
    assert!((pose.angles[1] - 2027).abs() <= 1, "yaw {}", pose.angles[1]);
    assert_eq!(pose.angles[2], 0, "roll");
}
