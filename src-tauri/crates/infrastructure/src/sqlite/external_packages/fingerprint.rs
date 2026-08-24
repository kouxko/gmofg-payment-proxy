//! 外部协议包注册合同的规范化指纹。

use intercept_proxy_domain::ExternalPackageRegistration;

use super::{InfrastructureError, corrupt_external_package};

/// 对严格注册对象的稳定 JSON 表达计算 SHA-256 指纹。
pub(crate) fn canonical_external_registration_fingerprint(
    registration: &ExternalPackageRegistration,
) -> Result<[u8; 32], InfrastructureError> {
    let json = canonical_external_registration_json(registration)?;
    Ok(sha256(json.as_bytes()))
}

pub(super) fn canonical_external_registration_json(
    registration: &ExternalPackageRegistration,
) -> Result<String, InfrastructureError> {
    serde_json::to_string(registration).map_err(registration_serialization_error)
}

pub(super) fn registration_serialization_error(error: serde_json::Error) -> InfrastructureError {
    let message = format!("注册合同无法规范化序列化：{error}");
    // `Result::map_err` 按值交付错误；明确消费所有权，避免该边界被误读为可借用回调。
    drop(error);
    corrupt_external_package(message)
}

pub(super) fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut fingerprint = [0_u8; 32];
    fingerprint.copy_from_slice(digest.as_ref());
    fingerprint
}
