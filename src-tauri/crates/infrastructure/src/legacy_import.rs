//! Bounded one-time pending storage for legacy JSON import confirmation.

use std::{
    collections::{BTreeMap, btree_map::Entry},
    fmt,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, LegacyImportCandidate, LegacyImportPreparePort, LegacyImportToken,
};
use parking_lot::Mutex;
use uuid::Uuid;

const LEGACY_IMPORT_TTL: Duration = Duration::from_mins(15);
const LEGACY_IMPORT_CAPACITY: usize = 8;

pub struct LegacyImportPreparer {
    pending: Mutex<BTreeMap<LegacyImportToken, PendingLegacyImport>>,
}

struct PendingLegacyImport {
    expires_at: Instant,
    candidate: LegacyImportCandidate,
}

impl LegacyImportPreparer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(BTreeMap::new()),
        }
    }

    fn remove_expired(pending: &mut BTreeMap<LegacyImportToken, PendingLegacyImport>) {
        let now = Instant::now();
        pending.retain(|_, value| value.expires_at > now);
    }
}

impl Default for LegacyImportPreparer {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for LegacyImportPreparer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LegacyImportPreparer")
            .field("pending_count", &self.pending.lock().len())
            .finish()
    }
}

#[async_trait]
impl LegacyImportPreparePort for LegacyImportPreparer {
    async fn retain(
        &self,
        candidate: LegacyImportCandidate,
    ) -> AppResult<(LegacyImportToken, Duration)> {
        let mut pending = self.pending.lock();
        Self::remove_expired(&mut pending);
        if pending.len() >= LEGACY_IMPORT_CAPACITY {
            return Err(AppError::new(
                "LEGACY_IMPORT_CAPACITY",
                "待确认的旧版导入数量已达上限。",
            ));
        }
        let token = LegacyImportToken::from_uuid(Uuid::new_v4());
        match pending.entry(token) {
            Entry::Vacant(entry) => entry.insert(PendingLegacyImport {
                expires_at: Instant::now() + LEGACY_IMPORT_TTL,
                candidate,
            }),
            Entry::Occupied(_) => {
                return Err(AppError::new(
                    "LEGACY_IMPORT_TOKEN_COLLISION",
                    "无法安全生成旧版导入确认令牌。",
                ));
            }
        };
        Ok((token, LEGACY_IMPORT_TTL))
    }

    async fn take(&self, token: LegacyImportToken) -> AppResult<LegacyImportCandidate> {
        let entry = self
            .pending
            .lock()
            .remove(&token)
            .ok_or_else(invalid_token)?;
        if entry.expires_at <= Instant::now() {
            return Err(AppError::new(
                "LEGACY_IMPORT_TOKEN_EXPIRED",
                "旧版导入确认令牌已过期。",
            ));
        }
        Ok(entry.candidate)
    }

    async fn discard(&self, token: LegacyImportToken) -> AppResult<()> {
        self.pending
            .lock()
            .remove(&token)
            .map(|_| ())
            .ok_or_else(invalid_token)
    }
}

fn invalid_token() -> AppError {
    AppError::new("LEGACY_IMPORT_TOKEN_INVALID", "旧版导入确认令牌无效。")
}

#[cfg(test)]
mod tests {
    use intercept_proxy_application::{
        LegacyImportCandidate, LegacyImportPreparePort, MigrationReport, MigrationSourceKind,
        WORKSPACE_DOCUMENT_FORMAT_VERSION, WorkspaceDocument,
    };
    use intercept_proxy_domain::ProxyWorkspace;

    use super::LegacyImportPreparer;

    fn candidate() -> LegacyImportCandidate {
        LegacyImportCandidate::Workspace {
            source_version: 4,
            document: WorkspaceDocument {
                format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
                workspace: ProxyWorkspace::default(),
                certificate_materials: Vec::new(),
                protocol_packages: Vec::new(),
            },
            migration_report: MigrationReport::unchanged(MigrationSourceKind::WorkspaceDocument, 4),
        }
    }

    #[tokio::test]
    async fn token_is_consumed_exactly_once() {
        let pending = LegacyImportPreparer::new();
        let (token, _) = pending.retain(candidate()).await.unwrap();
        pending.take(token).await.expect("first take");
        assert_eq!(
            pending.take(token).await.unwrap_err().view_model.code,
            "LEGACY_IMPORT_TOKEN_INVALID"
        );
    }

    #[tokio::test]
    async fn discarded_token_cannot_commit() {
        let pending = LegacyImportPreparer::new();
        let (token, _) = pending.retain(candidate()).await.unwrap();
        pending.discard(token).await.unwrap();
        assert_eq!(
            pending.take(token).await.unwrap_err().view_model.code,
            "LEGACY_IMPORT_TOKEN_INVALID"
        );
    }
}
