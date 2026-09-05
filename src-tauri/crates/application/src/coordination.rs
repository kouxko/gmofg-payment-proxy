/// Serializes configuration writers while allowing Android activation to hold a read lease.
#[derive(Debug, Default)]
pub(crate) struct ConfigurationMutationCoordinator(tokio::sync::RwLock<()>);

impl ConfigurationMutationCoordinator {
    pub(crate) async fn lock(&self) -> tokio::sync::RwLockWriteGuard<'_, ()> {
        self.0.write().await
    }

    pub(crate) async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, ()> {
        self.0.read().await
    }
}
