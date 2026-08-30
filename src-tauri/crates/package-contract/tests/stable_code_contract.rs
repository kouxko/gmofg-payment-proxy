use intercept_proxy_domain::ErrorCode;
use intercept_proxy_package_contract::{PackageRpcError, PackageRpcErrorData};

#[test]
fn rpc_error_data_exposes_domain_stable_code_without_message_parsing() {
    let error = PackageRpcError::new(-32000, "decode failed", ErrorCode::BodyDecodeFailed);
    let value = serde_json::to_value(error).expect("serialize");
    assert_eq!(value["data"]["code"], "BODY_DECODE_FAILED");
    assert_eq!(
        serde_json::from_value::<PackageRpcErrorData>(value["data"].clone())
            .expect("data")
            .code(),
        ErrorCode::BodyDecodeFailed
    );
}
