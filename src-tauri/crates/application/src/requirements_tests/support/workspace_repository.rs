use super::*;

#[derive(Debug, Default)]
pub(in crate::requirements_tests) struct SnapshotProbeWorkspaceRepository {
    inner: InMemoryWorkspaceStore,
    pub(in crate::requirements_tests) snapshot_calls: AtomicUsize,
    pub(in crate::requirements_tests) list_calls: AtomicUsize,
    pub(in crate::requirements_tests) get_calls: AtomicUsize,
    pub(in crate::requirements_tests) block_snapshot: AtomicBool,
    pub(in crate::requirements_tests) snapshot_entered: tokio::sync::Notify,
    pub(in crate::requirements_tests) continue_snapshot: tokio::sync::Notify,
}

#[async_trait]
impl WorkspaceRepositoryPort for SnapshotProbeWorkspaceRepository {
    async fn snapshot(&self) -> AppResult<WorkspaceCollectionViewModel> {
        self.snapshot_calls.fetch_add(1, Ordering::SeqCst);
        if self.block_snapshot.load(Ordering::SeqCst) {
            self.snapshot_entered.notify_one();
            self.continue_snapshot.notified().await;
        }
        self.inner.snapshot().await
    }

    async fn list(&self) -> AppResult<Vec<WorkspaceSummaryViewModel>> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.list().await
    }

    async fn get(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        self.get_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.get(workspace_id).await
    }

    async fn create(&self, name: String) -> AppResult<ProxyWorkspace> {
        self.inner.create(name).await
    }

    async fn copy(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        self.inner.copy(workspace_id).await
    }

    async fn select(&self, workspace_id: WorkspaceId) -> AppResult<WorkspaceSummaryViewModel> {
        self.inner.select(workspace_id).await
    }

    async fn validate(&self, workspace: ProxyWorkspace) -> AppResult<WorkspaceValidationViewModel> {
        self.inner.validate(workspace).await
    }

    async fn save(&self, workspace: ProxyWorkspace) -> AppResult<ProxyWorkspace> {
        self.inner.save(workspace).await
    }

    async fn import_workspace(&self, workspace: ProxyWorkspace) -> AppResult<ProxyWorkspace> {
        self.inner.import_workspace(workspace).await
    }

    async fn delete(
        &self,
        workspace_id: WorkspaceId,
        expected_revision: u64,
    ) -> AppResult<OperationResultViewModel> {
        self.inner.delete(workspace_id, expected_revision).await
    }
}
