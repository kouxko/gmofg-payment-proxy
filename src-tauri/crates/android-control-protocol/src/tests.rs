use super::*;
use serde_json::json;

#[test]
fn v2_round_trip_keeps_operation_identity() {
    for operation in [
        "profile_list",
        "profile_get",
        "profile_save",
        "profile_delete",
        "start",
        "apply",
        "apply_active",
        "stop",
        "emergency_restore",
        "status",
        "heartbeat",
    ] {
        let request = AndroidControlRequest::new(operation, json!({})).unwrap();
        assert_eq!(request.version, 2);
        let frame = encode_android_control_frame(&request).unwrap();
        assert_eq!(
            decode_android_control_frame::<AndroidControlRequest>(&frame).unwrap(),
            request
        );
    }
    assert!(matches!(
        AndroidControlRequest::new("unknown", json!({})),
        Err(AndroidControlError::InvalidOperation)
    ));
}

#[test]
fn framing_accepts_exact_limit_and_rejects_extra_byte_or_truncation() {
    let text = "x".repeat(ANDROID_CONTROL_MAX_FRAME_BYTES - 2);
    let frame = encode_android_control_frame(&text).unwrap();
    assert_eq!(frame.len(), ANDROID_CONTROL_MAX_FRAME_BYTES + 4);
    assert_eq!(&frame[..4], &1_048_576_u32.to_be_bytes());
    assert_eq!(
        decode_android_control_frame::<String>(&frame).unwrap(),
        text
    );
    assert!(matches!(
        decode_android_control_frame::<Value>(&frame[..frame.len() - 1]),
        Err(AndroidControlError::InvalidLength)
    ));
    assert!(matches!(
        encode_android_control_frame(&(text + "x")),
        Err(AndroidControlError::FrameTooLarge)
    ));
    assert!(matches!(
        decode_android_control_frame::<Value>(&[0, 0]),
        Err(AndroidControlError::MissingLengthPrefix)
    ));
    assert!(matches!(
        decode_android_control_frame::<Value>(&[0, 0, 0, 1, b'{']),
        Err(AndroidControlError::InvalidJson(_))
    ));
}

#[test]
fn request_constructor_counts_envelope_bytes_in_limit() {
    let request = AndroidControlRequest::new("status", json!("")).unwrap();
    let overhead = serde_json::to_vec(&request).unwrap().len();
    let text = "x".repeat(ANDROID_CONTROL_MAX_FRAME_BYTES - overhead);
    let exact = AndroidControlRequest::new("status", json!(text)).unwrap();
    assert_eq!(
        serde_json::to_vec(&exact).unwrap().len(),
        ANDROID_CONTROL_MAX_FRAME_BYTES
    );
    assert!(matches!(
        AndroidControlRequest::new("status", json!(text + "x")),
        Err(AndroidControlError::RequestTooLarge)
    ));
}
