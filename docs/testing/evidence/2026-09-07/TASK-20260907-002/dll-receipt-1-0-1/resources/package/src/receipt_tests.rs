use serde_json::{Value, json};

use super::{decode_downstream, display_downstream, encode_downstream};

const RECEIPTS: [&str; 4] = [
    include_str!("../tests/fixtures/receipt-credit-success.json"),
    include_str!("../tests/fixtures/receipt-credit-failure.json"),
    include_str!("../tests/fixtures/receipt-union-pay-success.json"),
    include_str!("../tests/fixtures/receipt-union-pay-failure.json"),
];

#[test]
fn observed_receipt_log_bodies_decode_without_losing_logged_fields() {
    // These are lossy UTF-8 log strings, not the original Shift-JIS wire bytes.
    let bodies: Vec<Value> =
        serde_json::from_str(include_str!("../tests/fixtures/receipt-observed-log.json")).unwrap();
    assert_eq!(bodies.len(), 19);
    for body in bodies {
        let input = body.to_string();
        let document = decode_downstream(&input).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&document).unwrap(), body);
        assert_eq!(encode_downstream(&input, &document).unwrap(), input);
    }
}

#[test]
fn receipt_success_and_failure_preserve_fields_and_original_text() {
    for input in RECEIPTS {
        let document = decode_downstream(input).expect("decode receipt");
        let expected: Value = serde_json::from_str(input).unwrap();
        let actual: Value = serde_json::from_str(&document).unwrap();
        assert_eq!(actual, expected);
        assert!(actual.get("ResponseTimeStamp").is_none());
        assert_eq!(encode_downstream(input, &document).unwrap(), input);
        let html = display_downstream(&document).unwrap();
        assert!(html.contains("ErrorMessage"));
        assert!(html.contains(actual["ErrorMessage"].as_str().unwrap()));
    }
}

#[test]
fn edited_receipt_reencodes_and_escapes_display_without_changing_error_semantics() {
    for input in RECEIPTS {
        let document = decode_downstream(input).unwrap();
        let mut changed: Value = serde_json::from_str(&document).unwrap();
        changed["ErrorCode"] = json!("D48");
        changed["ErrorMessage"] = json!("送信失敗 <script>alert('x')</script> & 確認");
        let encoded = encode_downstream(input, &changed.to_string()).unwrap();
        let actual: Value = serde_json::from_str(&decode_downstream(&encoded).unwrap()).unwrap();
        assert_eq!(actual, changed);
        let html = display_downstream(&changed.to_string()).unwrap();
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("D48"));
    }
}

#[test]
fn receipt_invalid_fields_and_cross_protocol_tables_fail_in_all_hooks() {
    let input = RECEIPTS[0];
    let original: Value = serde_json::from_str(input).unwrap();
    let mut invalid = vec![json!(null), json!([]), json!({})];
    for field in [
        "ResponseID",
        "TransactionType",
        "ResponseDateTime",
        "ErrorCode",
        "ErrorMessage",
    ] {
        let mut missing = original.clone();
        missing.as_object_mut().unwrap().remove(field);
        invalid.push(missing);
        for value in [json!(null), json!(123), json!({})] {
            let mut wrong_type = original.clone();
            wrong_type[field] = value;
            invalid.push(wrong_type);
        }
    }
    for (field, value) in [
        ("TransactionType", json!("U012")),
        ("TransactionType", json!("0001")),
        ("Unexpected", json!("value")),
        ("KCCI_01", json!({"Length":"1","Data":"X"})),
    ] {
        let mut changed = original.clone();
        changed[field] = value;
        invalid.push(changed);
    }
    for value in invalid {
        let text = value.to_string();
        assert!(decode_downstream(&text).is_err(), "decode accepted {text}");
        assert!(
            encode_downstream(input, &text).is_err(),
            "encode accepted {text}"
        );
        assert!(
            display_downstream(&text).is_err(),
            "display accepted {text}"
        );
    }
    let duplicate = input.replacen('{', "{\"ErrorCode\":\"000\",", 1);
    assert!(decode_downstream(&duplicate).is_err());
    assert!(encode_downstream(input, &duplicate).is_err());
    assert!(display_downstream(&duplicate).is_err());
}
