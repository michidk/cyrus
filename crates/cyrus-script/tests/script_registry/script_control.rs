use super::*;

#[test]
fn hook_toggles_set_and_clear_only_the_native_hook_suppression_bit() {
    let hook_bit = |calls: &[(Native, &[i32])]| {
        let world = run_native_world(calls).unwrap();
        let control = world.actors()[0].control_state().unwrap();
        (
            control.script_control.bits(),
            world.actors()[1]
                .control_state()
                .unwrap()
                .script_control
                .bits(),
        )
    };
    assert_eq!(hook_bit(&[(Native::DisableHook, &[])]), (2, 0));
    assert_eq!(
        hook_bit(&[(Native::DisableHook, &[]), (Native::EnableHook, &[])]),
        (0, 0)
    );
    assert_eq!(hook_bit(&[(Native::EnableHook, &[])]), (0, 0));
}

#[test]
fn enable_and_disable_natives_toggle_their_script_control_bit() {
    let bits = |calls: &[(Native, &[i32])]| {
        run_native_world(calls).unwrap().actors()[0]
            .control_state()
            .unwrap()
            .script_control
            .bits()
    };
    for (disable, enable, bit) in [
        (Native::DisableScript, Native::EnableScript, 1),
        (Native::DisableTasks, Native::EnableTasks, 4),
        (Native::DisableObject, Native::EnableObject, 8),
        (Native::DisableCombat, Native::EnableCombat, 16),
    ] {
        assert_eq!(bits(&[(disable, &[])]), bit, "{disable}");
        assert_eq!(bits(&[(disable, &[]), (enable, &[])]), 0, "{enable}");
    }
    assert_eq!(
        bits(&[
            (Native::DisableTasks, &[]),
            (Native::DisableCombat, &[]),
            (Native::EnableTasks, &[])
        ]),
        16
    );
}
