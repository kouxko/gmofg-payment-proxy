//! `SQLite` header 到无源码注册表摘要的单向投影。

use super::{
    ProtocolPackageSummary, ProtocolPackageValidationStatus, StoredProtocolPackageHeader,
    StoredProtocolPackageValidation,
};

pub(super) fn summary_from_header(header: StoredProtocolPackageHeader) -> ProtocolPackageSummary {
    let validation = match header.validation {
        StoredProtocolPackageValidation::Valid => ProtocolPackageValidationStatus::Valid,
        StoredProtocolPackageValidation::Invalid(code) => {
            ProtocolPackageValidationStatus::Invalid { code }
        }
    };
    ProtocolPackageSummary {
        package: header.package,
        name: header.name,
        host_api: header.host_api,
        enabled: header.enabled,
        validation,
        installed_at: header.installed_at,
    }
}
