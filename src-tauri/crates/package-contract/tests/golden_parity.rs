use intercept_proxy_domain::Document;
use intercept_proxy_package_contract::{
    FrameResult, PackageManifest, PackageRegisterNotification, PackageRpcFailure,
    PackageRpcRequest, PackageRpcSuccess,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

const HTTP: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/http-manifest.json"
);
const SOCKET: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/socket-manifest.json"
);
const REGISTER: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/register-notification.json"
);
const RPC: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/rpc-examples.json"
);
const GOLDEN: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/golden.json"
);

fn round_trip<T>(wire: &Value) -> T
where
    T: DeserializeOwned + Serialize,
{
    let parsed = serde_json::from_value::<T>(wire.clone()).expect("contract input");
    assert_eq!(
        serde_json::to_value(&parsed).expect("contract output"),
        *wire
    );
    parsed
}

#[test]
fn independent_json_fixtures_round_trip_through_the_rust_source_contract() {
    for manifest in [HTTP, SOCKET] {
        let parsed: PackageManifest = serde_json::from_str(manifest).expect("manifest");
        assert_eq!(
            serde_json::to_value(parsed).expect("serialize"),
            serde_json::from_str::<Value>(manifest).expect("fixture")
        );
    }
    let registration = serde_json::from_str::<Value>(REGISTER).expect("notification fixture");
    round_trip::<PackageRegisterNotification>(&registration);
    let rpc: Value = serde_json::from_str(RPC).expect("fixture");
    for request in rpc["requests"].as_array().expect("requests") {
        round_trip::<PackageRpcRequest>(request);
    }
}

#[test]
fn canonical_golden_round_trips_every_request_result_and_error_shape() {
    let golden: Value = serde_json::from_str(GOLDEN).expect("golden");
    round_trip::<PackageManifest>(&golden["manifest"]);
    round_trip::<PackageRegisterNotification>(&golden["registration"]);
    let requests = golden["requests"].as_array().expect("requests");
    assert_eq!(requests.len(), 8);
    for request in requests {
        round_trip::<PackageRpcRequest>(request);
    }
    for response in golden["successes"]["frame"]
        .as_array()
        .expect("frame responses")
    {
        round_trip::<PackageRpcSuccess<FrameResult>>(response);
    }
    round_trip::<PackageRpcSuccess<Document>>(&golden["successes"]["decode"]);
    round_trip::<PackageRpcSuccess<String>>(&golden["successes"]["encode"]);
    round_trip::<PackageRpcSuccess<String>>(&golden["successes"]["display"]);
    let failure = round_trip::<PackageRpcFailure>(&golden["failure"]);
    assert_eq!(failure.error.data().code().as_str(), "BODY_DECODE_FAILED");
}
