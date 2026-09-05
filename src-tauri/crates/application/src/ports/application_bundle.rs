use crate::{AppResult, ApplicationConfigurationDocument, PortableApplicationProtocolPackage};
use async_trait::async_trait;

#[async_trait]
/// Sole application-facing writer for the complete persisted configuration bundle.
pub trait ApplicationBundleReplacementPort: Send + Sync + std::fmt::Debug {
    async fn replace_application_bundle(
        &self,
        packages: Vec<PortableApplicationProtocolPackage>,
        document: ApplicationConfigurationDocument,
    ) -> AppResult<()>;
    async fn reset_application_bundle(
        &self,
        document: ApplicationConfigurationDocument,
    ) -> AppResult<()>;
}
