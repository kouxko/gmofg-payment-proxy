//! 可复用代理核心与宿主装配之间的策略边界。
//!
//! 本 crate 只定义稳定契约。应用名称、默认入口、安全命名空间与可选策略由宿主装配
//! 提供，不能泄漏进通用领域层或代理运行时。

mod certificates;
mod codec;
mod contracts;
mod error;
mod intercept_profile;
mod profile;
mod validation;

pub use certificates::{CertificateLabels, ProductCertificatePolicy};
pub use codec::BodyCodec;
pub use contracts::{
    ClassifiedRequest, LegacySettingsChannelMapping, ProductChannel, ProductFaultTemplate,
    ProductHeader, ProductLabels, ProductMessageContext, ProductPersistenceMigrations,
    ProductStorageNamespace, RequestClassifier, STANDARD_FAULT_CAPABILITY_IDS,
};
pub use error::ProductError;
pub use intercept_profile::InterceptProxyProfile;
pub use profile::ProductProfile;
pub use validation::validate_product_profile;

#[cfg(test)]
mod tests;
