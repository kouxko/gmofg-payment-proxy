use intercept_proxy_package_contract::{
    PackageRegisterNotification, PackageRpcFailure, PackageRpcRequest, PackageRpcSuccess,
};
use serde_json::{Value, json};

const REGISTER: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/register-notification.json"
);
const RPC: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/rpc-examples.json"
);

#[test]
fn package_register_is_an_id_less_one_way_notification() {
    let notification: PackageRegisterNotification =
        serde_json::from_str(REGISTER).expect("valid registration notification");
    assert_eq!(
        serde_json::to_value(notification).expect("serialize"),
        serde_json::from_str::<Value>(REGISTER).expect("fixture")
    );
    let mut with_id: Value = serde_json::from_str(REGISTER).expect("fixture");
    with_id["id"] = json!("register-1");
    assert!(serde_json::from_value::<PackageRegisterNotification>(with_id).is_err());
}

#[test]
fn fixed_hook_requests_use_string_ids_exact_methods_and_camel_case_params() {
    let fixture: Value = serde_json::from_str(RPC).expect("fixture");
    for request in fixture["requests"].as_array().expect("requests") {
        let parsed: PackageRpcRequest =
            serde_json::from_value(request.clone()).expect("valid request");
        assert_eq!(serde_json::to_value(parsed).expect("serialize"), *request);
    }
    for invalid in [
        json!({"jsonrpc":"2.0","id":7,"method":"hooks.upstream.frame","params":{"buffer":"AA=="}}),
        json!({"jsonrpc":"2.0","id":"x","method":"hooks.unknown","params":{}}),
        json!({"jsonrpc":"2.0","id":"x","method":"hooks.upstream.frame","params":{"buffer":"AA","extra":true}}),
        json!({"jsonrpc":"2.0","id":"x","method":"hooks.upstream.encode","params":{"original_input":"x","document":null}}),
    ] {
        assert!(serde_json::from_value::<PackageRpcRequest>(invalid).is_err());
    }
}

#[test]
fn rpc_success_and_failure_envelopes_are_strict_and_mutually_exclusive() {
    let fixture: Value = serde_json::from_str(RPC).expect("fixture");
    serde_json::from_value::<PackageRpcSuccess<Value>>(fixture["success"].clone())
        .expect("success");
    serde_json::from_value::<PackageRpcFailure>(fixture["failure"].clone()).expect("failure");
    assert!(
        serde_json::from_value::<PackageRpcSuccess<Value>>(
            json!({"jsonrpc":"2.0","id":"x","result":{},"error":{}})
        )
        .is_err()
    );
}
