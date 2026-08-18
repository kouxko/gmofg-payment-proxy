use super::*;

#[derive(Debug)]
pub(super) struct FakeBackupPrepareSource {
    candidate: ApplicationBackupImportCandidate,
    pub(super) retain_calls: AtomicUsize,
    pub(super) discard_calls: AtomicUsize,
    pub(super) retained: parking_lot::Mutex<Vec<PreparedApplicationBackup>>,
}

impl FakeBackupPrepareSource {
    pub(super) fn new(candidate: ApplicationBackupImportCandidate) -> Self {
        Self {
            candidate,
            retain_calls: AtomicUsize::new(0),
            discard_calls: AtomicUsize::new(0),
            retained: parking_lot::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ApplicationBackupImportPreparePort for FakeBackupPrepareSource {
    async fn read(&self, _: Vec<u8>) -> AppResult<ApplicationBackupImportCandidate> {
        Ok(self.candidate.clone())
    }

    async fn retain(
        &self,
        prepared: PreparedApplicationBackup,
    ) -> AppResult<(ApplicationBackupImportToken, Duration)> {
        self.retain_calls.fetch_add(1, Ordering::SeqCst);
        self.retained.lock().push(prepared);
        Ok((
            ApplicationBackupImportToken::from_uuid(uuid::Uuid::from_u128(77)),
            Duration::from_mins(5),
        ))
    }

    async fn discard(&self, _: ApplicationBackupImportToken) -> AppResult<()> {
        self.discard_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn take(&self, _: ApplicationBackupImportToken) -> AppResult<PreparedApplicationBackup> {
        self.retained.lock().pop().ok_or_else(|| {
            AppError::new(
                "APPLICATION_BACKUP_IMPORT_TOKEN_INVALID",
                "测试应用备份令牌无效。",
            )
        })
    }
}
