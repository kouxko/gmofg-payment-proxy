//! 外部协议包规范身份指纹及稳定应用错误。

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_package_contract::PackageManifest;

use crate::sqlite::external_packages::canonical_external_registration_fingerprint;

/// 对规范化注册合同计算稳定 SHA-256 指纹。
pub fn external_package_registration_fingerprint(
    registration: &PackageManifest,
) -> AppResult<[u8; 32]> {
    canonical_external_registration_fingerprint(registration).map_err(|error| {
        tracing::error!(error = ?error, "external package registration serialization failed");
        AppError::new("INTERNAL_ERROR", "外部协议包注册内容无法规范化。")
    })
}

pub(super) fn not_found(package: &ProtocolPackageRef) -> AppError {
    package_error(
        "PROTOCOL_PACKAGE_NOT_FOUND",
        "外部协议包精确版本不存在。",
        package,
    )
}

pub(super) fn package_error(code: &str, message: &str, package: &ProtocolPackageRef) -> AppError {
    AppError::new(code, message).entity(format!("{}@{}", package.id, package.version))
}
