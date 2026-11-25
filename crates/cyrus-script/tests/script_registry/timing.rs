use super::*;

#[test]
fn speed_scale_scales_by_the_frame_scale_and_mainloop_exit_keeps_codes_from_5000() {
    // The helper's frame scale is 256, which maps values to themselves with the sign kept.
    assert_eq!(
        run_native_calls(&[(Native::SpeedScale, &[1234])]).unwrap(),
        1234
    );
    assert_eq!(
        run_native_calls(&[(Native::SpeedScale, &[-300])]).unwrap(),
        -300
    );
    let exit = |code: i32| {
        let mut world = run_native_world(&[(Native::MainloopExit, &[code])]).unwrap();
        world.globals_mut().take_host_requests().mainloop_exit
    };
    assert_eq!(exit(4999), None);
    assert_eq!(exit(5000), Some(5000));
    assert_eq!(exit(-7), None, "the comparison is signed");
    assert!(run_native_world(&[(Native::SpeedScale, &[])]).is_err());
}

#[test]
fn frame_passed_reports_the_shared_nominal_tick_increment() {
    assert_eq!(
        run_native_calls(&[(Native::GetFramePassed, &[])]).unwrap(),
        3
    );
}
