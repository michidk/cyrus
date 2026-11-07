//! `FlatSetTexture` and `FlatStop` through the public native dispatch, from the published record rules.
use super::*;
use cyrus_script::script::{ScriptInputs, ScriptStage};

/// Actor 0 runs the given flat natives as statements over installed textures bank 2 image 3 (five
/// frames) and bank 1 image 1 (four frames).
fn run(calls: &[(Native, &[i32])]) -> (ScriptWorld, anyhow::Result<()>) {
    let mut scripts = wait_cache(
        calls
            .iter()
            .enumerate()
            .map(|(index, (native, arguments))| {
                wait(
                    index as u16 + 1,
                    *native,
                    CallKind::Function,
                    arguments
                        .iter()
                        .map(|&value| Value::Integer { value })
                        .collect(),
                )
            })
            .chain([Operation::End { target: 0 }])
            .collect(),
    );
    scripts.definition.functions.truncate(1);
    for (native, arguments) in calls {
        scripts.definition.functions.push(Function {
            name: *native,
            param_count: arguments.len() as u8,
            kind: CallKind::Function,
        });
    }
    scripts.texture_frames.insert(2, BTreeMap::from([(3, 5)]));
    scripts.texture_frames.insert(1, BTreeMap::from([(1, 4)]));
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let result = world.advance(
        id,
        ScriptStage::Body(Thread::Main),
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        },
        None,
        None,
        100,
    );
    (world, result)
}

#[test]
fn flat_set_texture_forms_the_identifier_from_the_bank_and_image_and_enables_the_record() {
    // ((bank << 7) & 0xff80) | (image & 7): bank 2 image 3 is 259; bank 1 image 1 is 129.
    for (bank, image, texture) in [(2, 3, 259), (1, 1, 129)] {
        let (world, result) = run(&[(Native::FlatSetTexture, &[bank, image])]);
        result.unwrap();
        let flat = world.actors()[0].flat_state().unwrap();
        assert_eq!(flat["texture"], texture);
        assert_eq!(flat["enabled"], true);
        assert_eq!(flat["pending"], 0);
    }
}

#[test]
fn flat_set_texture_creates_the_record_and_fails_on_an_uninstalled_image() {
    // Uninstalled image: the original's failed load is fatal; cyrus names the missing image.
    let (world, result) = run(&[(Native::FlatSetTexture, &[7, 3])]);
    let error = format!("{:#}", result.unwrap_err());
    assert!(error.contains("not installed"), "{error}");
    assert!(world.actors()[0].failure().is_some());
    // Images above seven stay explicitly unsupported rather than guessed.
    let (_, result) = run(&[(Native::FlatSetTexture, &[2, 8])]);
    let error = format!("{:#}", result.unwrap_err());
    assert!(error.contains("above seven unsupported"), "{error}");
}

#[test]
fn flat_stop_enables_the_record_and_clears_its_pending_and_animation_bytes() {
    let (world, result) = run(&[
        (Native::FlatSetTexture, &[2, 3]),
        (Native::FlatOff, &[]),
        (Native::FlatStop, &[]),
    ]);
    result.unwrap();
    let flat = world.actors()[0].flat_state().unwrap();
    assert_eq!(flat["enabled"], true);
    assert_eq!(flat["pending"], 0);
    // It also creates a record when the receiver has none.
    let (world, result) = run(&[(Native::FlatStop, &[])]);
    result.unwrap();
    assert_eq!(world.actors()[0].flat_state().unwrap()["enabled"], true);
}
