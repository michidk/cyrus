use cyrus_control::{Command, InputAction, Request, Response};

#[test]
fn request_roundtrip_and_wire_shape() {
    let req = Request {
        id: 7,
        cmd: Command::Input {
            key: "KeyW".into(),
            action: InputAction::Tap,
        },
    };
    let json = serde_json::to_string(&req).unwrap();
    assert_eq!(
        json,
        r#"{"id":7,"cmd":{"type":"input","key":"KeyW","action":"tap"}}"#
    );
    let back: Request = serde_json::from_str(&json).unwrap();
    assert_eq!(back.cmd, req.cmd);
}

#[test]
fn error_response_omits_result() {
    let json = serde_json::to_string(&Response::err(1, "nope")).unwrap();
    assert_eq!(json, r#"{"id":1,"ok":false,"error":"nope"}"#);
}

#[test]
fn startup_skip_has_no_world_parameter() {
    let request: Request =
        serde_json::from_str(r#"{"id":8,"cmd":{"type":"skip_startup"}}"#).unwrap();
    assert_eq!(request.cmd, Command::SkipStartup);
    assert_eq!(
        serde_json::to_string(&request).unwrap(),
        r#"{"id":8,"cmd":{"type":"skip_startup"}}"#
    );
}

#[test]
fn saved_images_read_requires_explicit_metadata_path() {
    let wire =
        r#"{"id":19,"cmd":{"type":"read_saved_images","savegame_path":"/fixtures/SAVEGAME.SAV"}}"#;
    let request: Request = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&request).unwrap(), wire);
    assert!(
        serde_json::from_str::<Request>(r#"{"id":19,"cmd":{"type":"read_saved_images"}}"#).is_err()
    );
}
