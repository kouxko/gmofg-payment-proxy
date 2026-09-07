use serde_json::{Value, json};

use super::{Cps, Guest, codec, finalize, route, signing};
use route::Operation;

#[test]
fn signatures_match_original_java_oracle() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../tests/fixtures/java-signatures.json")).unwrap();
    for case in cases {
        let operation = match case["operation"].as_str().unwrap() {
            "pay" => Operation::Pay,
            "confirm" => Operation::Confirm,
            "refund" => Operation::Refund,
            "reverse" => Operation::Reverse,
            other => panic!("unknown operation {other}"),
        };
        let body: Value = serde_json::from_str(case["body"].as_str().unwrap()).unwrap();
        let token = case["token"].as_str().unwrap();
        let canonical = signing::canonical(&body, operation, token);
        let sign = signing::signature(&body, operation, token);
        if case["expect_error"] == true {
            assert!(canonical.is_none(), "{}", case["id"]);
            assert!(sign.is_none(), "{}", case["id"]);
        } else {
            assert_eq!(
                canonical.as_deref(),
                case["canonical"].as_str(),
                "{}",
                case["id"]
            );
            assert_eq!(sign.as_deref(), case["sign"].as_str(), "{}", case["id"]);
        }
    }
}

fn response() -> Value {
    json!({
        "meta": {"code":"00", "message":"成功 OK"},
        "data": {"sign":"OLD", "errorInfo":"", "result": {
            "orderId":"order 1", "orderDetailId":"detail1", "transStatus":"SUCCESS",
            "transTime":"20260907120000", "currencyCode":"JPY", "amount":100,
            "amountRmb":0, "payType":"99", "extendedInfo":"unchanged"
        }},
        "extension":{"keep":[1,true,null]}
    })
}

fn envelope(body: &str) -> Value {
    json!({
        "url":"https://cps.example/gateway/products-pointPay/version=1.0/B01/1/0/MERCHANT%20%20TERMINAL%20%20TOKEN",
        "method":"POST", "code":200,
        "header":{"Content-Type":"application/json;charset=UTF-8","X-Keep":"yes"},
        "body":body
    })
}

#[test]
fn downstream_decode_display_encode_preserves_or_edits_complete_json() {
    let original = format!("  {}\r\n", response());
    let document = Cps::downstream_decode(original.clone()).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&document).unwrap(),
        response()
    );
    assert_eq!(
        Cps::downstream_encode(original.clone(), document.clone()).unwrap(),
        original
    );
    let display = Cps::downstream_display(document.clone()).unwrap();
    for field in [
        "meta",
        "data",
        "result",
        "orderId",
        "amount",
        "sign",
        "extension",
    ] {
        assert!(display.contains(field), "missing {field}");
    }
    let mut changed: Value = serde_json::from_str(&document).unwrap();
    changed["data"]["result"]["amount"] = json!(250);
    let encoded = Cps::downstream_encode(original, changed.to_string()).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), changed);
    assert_eq!(
        changed["data"]["sign"], "OLD",
        "Encode does not sign before Finalize"
    );
}

#[test]
fn finalize_changes_only_data_sign_and_is_idempotent() {
    let before_body = response();
    let before = envelope(&before_body.to_string());
    let output = finalize::downstream(&before.to_string()).unwrap();
    let after: Value = serde_json::from_str(&output).unwrap();
    for field in ["url", "method", "code", "header"] {
        assert_eq!(after[field], before[field]);
    }
    let mut after_body: Value = serde_json::from_str(after["body"].as_str().unwrap()).unwrap();
    let sign = after_body["data"]["sign"].as_str().unwrap();
    assert_eq!(sign.len(), 32);
    assert!(
        sign.bytes()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
    );
    after_body["data"]["sign"] = json!("OLD");
    assert_eq!(after_body, before_body);
    assert_eq!(finalize::downstream(&output).unwrap(), output);
}

#[test]
fn business_inapplicability_returns_the_entire_input_exactly() {
    let valid = envelope(&response().to_string());
    let alterations = [
        ("method", json!("GET")),
        ("code", json!(400)),
        ("code", json!(null)),
        ("url", json!("/unknown/TOKEN")),
        ("url", json!("/gateway/pay/version=1.0/B01/1/0/")),
        ("url", json!("/gateway/pay/version=1.0/B01/1/0/%zz")),
        ("body", json!("")),
        ("body", json!("<html>bad gateway</html>")),
        ("body", json!("{}")),
        ("body", json!("{\"data\":{\"sign\":\"\"}}")),
        ("body", json!("{\"data\":{\"sign\":null}}")),
        ("body", json!("{\"data\":{\"sign\":\"OLD\",\"result\":[]}}")),
        (
            "body",
            json!("{\"data\":{\"sign\":\"OLD\",\"result\":{\"amount\":1.2}}}"),
        ),
    ];
    for (field, replacement) in alterations {
        let mut value = valid.clone();
        value[field] = replacement;
        let input = format!(" \n{}\n", value);
        assert_eq!(finalize::downstream(&input).unwrap(), input);
    }
}

#[test]
fn raw_body_and_html_are_safe_and_round_trip() {
    for input in ["", "not JSON", "<script>alert('x')&</script>"] {
        let decoded = codec::decode(input).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&decoded).unwrap(),
            json!(input)
        );
        assert_eq!(codec::encode(input, &decoded).unwrap(), input);
        let html = codec::display(&decoded).unwrap();
        assert!(!html.contains("<script>"));
    }
    let doc = json!({"<img onerror='bad'>":"<script>&\"'"}).to_string();
    let display = codec::display(&doc).unwrap();
    assert!(display.contains("&lt;img"));
    assert!(!display.contains("<img"));
    assert!(codec::display("invalid Document").is_err());
    assert!(codec::encode("{}", "invalid Document").is_err());
}

#[test]
fn malformed_http_wrapper_is_an_execution_error() {
    for input in ["not JSON", "{}", "[]"] {
        assert!(finalize::downstream(input).is_err());
    }
    let mut missing = envelope("{}");
    missing.as_object_mut().unwrap().remove("code");
    assert!(finalize::downstream(&missing.to_string()).is_err());
    let mut bad_header = envelope("{}");
    bad_header["header"]["bad"] = json!(["a", "b"]);
    assert!(finalize::downstream(&bad_header.to_string()).is_err());
}

#[test]
fn every_source_route_retains_raw_token_and_decodes_percent_once() {
    for (op, names, version) in [
        (
            Operation::Pay,
            [
                "stub-products-pointPay",
                "products-pointPay",
                "stubPay",
                "pay",
            ],
            "version=1.0/",
        ),
        (
            Operation::Confirm,
            [
                "stub-products-pointConfirm",
                "products-pointConfirm",
                "stubConfirm",
                "confirm",
            ],
            "version=1.0/",
        ),
        (
            Operation::Refund,
            [
                "stub-products-refund",
                "products-refund",
                "stubRefund",
                "refund",
            ],
            "",
        ),
        (
            Operation::Reverse,
            [
                "stub-products-reverse",
                "products-reverse",
                "stubReverse",
                "reverse",
            ],
            "",
        ),
    ] {
        for name in names {
            let path = format!("/base/gateway/{name}/{version}B01/1/0/A%20B+%2520?ignored=yes");
            assert_eq!(
                route::operation_and_token(&path),
                Some((op, "A B+%20".into()))
            );
            assert_eq!(
                route::operation_and_token(&format!("https://cps.example{path}")),
                Some((op, "A B+%20".into()))
            );
        }
    }
    assert_eq!(
        route::operation_and_token("/gateway/pay/version=1.0/B01/1/0/TOKEN/extra"),
        None
    );
    assert_eq!(
        route::operation_and_token("/gateway/pay/version=1.0/B01/1/0/%FF"),
        None
    );
    assert_eq!(
        route::operation_and_token(
            "https://cps.example?redirect=/gateway/pay/version=1.0/B01/1/0/T"
        ),
        None
    );
}

#[test]
fn signature_respects_java_model_projection_and_raw_token_padding() {
    let original = response();
    let canonical = signing::canonical(&original, Operation::Pay, " TOKEN ").unwrap();
    assert!(canonical.ends_with("&key=B2B TOKEN }"));
    assert!(canonical.contains("errorInfo="));
    assert!(canonical.contains("orderId=order1"));
    assert!(!canonical.contains("amountRmb="));
    assert!(!canonical.contains("sign="));
    assert!(!canonical.contains("extension="));
    let mut changed = original.clone();
    changed["data"]["result"]["amount"] = json!("100.0");
    assert_eq!(
        signing::signature(&original, Operation::Pay, "T"),
        signing::signature(&changed, Operation::Pay, "T")
    );
    assert_ne!(
        signing::signature(&original, Operation::Pay, "T"),
        signing::signature(&original, Operation::Pay, "T ")
    );
}
