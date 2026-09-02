use intercept_proxy_package_contract::FrameResult;
use serde_json::json;

#[test]
fn frame_result_accepts_all_three_closed_variants() {
    let values = [
        json!({"status":"need_more"}),
        json!({"status":"need_more","requiredBytes":4}),
        json!({"status":"complete","consumedBytes":7}),
        json!({"status":"reject","reason":"invalid frame"}),
    ];
    for value in values {
        let result: FrameResult = serde_json::from_value(value.clone()).expect("valid result");
        assert_eq!(serde_json::to_value(result).expect("serialize"), value);
    }
}

#[test]
fn frame_result_rejects_zero_complete_cross_fields_and_unknown_fields() {
    for invalid in [
        json!({"status":"complete","consumedBytes":0}),
        json!({"status":"complete","consumedBytes":1,"reason":"x"}),
        json!({"status":"need_more","consumedBytes":1}),
        json!({"status":"reject"}),
        json!({"status":"reject","reason":"x","extra":true}),
    ] {
        assert!(serde_json::from_value::<FrameResult>(invalid).is_err());
    }
}

#[test]
fn complete_result_is_validated_at_construction_and_against_buffer_length() {
    assert!(FrameResult::complete(0).is_err());
    let complete = FrameResult::complete(2).expect("positive consumed bytes");
    complete
        .validate_against_buffer_len(2)
        .expect("exact buffer length");
    assert!(complete.validate_against_buffer_len(1).is_err());
}
