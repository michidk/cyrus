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
