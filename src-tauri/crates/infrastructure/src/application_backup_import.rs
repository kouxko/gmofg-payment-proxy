//! Strict backup reconstruction and bounded non-authoritative pending storage.

use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    sync::Arc,
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use intercept_proxy_application::{
    APPLICATION_CONFIGURATION_FORMAT_VERSION, AppError, AppResult,
    ApplicationBackupImportCandidate, ApplicationBackupImportPreparePort,
    ApplicationBackupImportToken, MigrationReport, MigrationSourceKind,
    PortableApplicationProtocolPackage, PortableCertificateMaterial, PortableProtocolPackageFile,
    PreparedApplicationBackup, portable_material_sha256,
};
use parking_lot::Mutex;
use uuid::Uuid;

use crate::{ApplicationBackupArchive, ApplicationBackupArchiveLimits};

pub const DEFAULT_APPLICATION_BACKUP_PENDING_CAPACITY: usize = 8;
/// One maximum R07a uncompressed candidate can expand by 4/3 when reconstructed
/// as canonical Base64; 768 MiB bounds that single candidate plus typed config.
pub const DEFAULT_APPLICATION_BACKUP_PENDING_BYTES: u64 = 768 * 1024 * 1024;
pub const DEFAULT_APPLICATION_BACKUP_PENDING_TTL: Duration = Duration::from_mins(15);

pub trait ApplicationBackupImportClock: Send + Sync + fmt::Debug {
    fn now(&self) -> Duration;
}

pub trait ApplicationBackupImportTokenGenerator: Send + Sync + fmt::Debug {
    fn generate(&self) -> ApplicationBackupImportToken;
}

#[derive(Debug, Default)]
pub struct SystemApplicationBackupImportClock;

impl ApplicationBackupImportClock for SystemApplicationBackupImportClock {
    fn now(&self) -> Duration {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
    }
}

#[derive(Debug, Default)]
pub struct RandomApplicationBackupImportTokenGenerator;

impl ApplicationBackupImportTokenGenerator for RandomApplicationBackupImportTokenGenerator {
    fn generate(&self) -> ApplicationBackupImportToken {
        ApplicationBackupImportToken::from_uuid(Uuid::new_v4())
    }
}

pub struct ApplicationBackupImportPreparer {
    limits: ApplicationBackupArchiveLimits,
    ttl: Duration,
    capacity: usize,
    max_pending_bytes: u64,
    clock: Arc<dyn ApplicationBackupImportClock>,
    tokens: Arc<dyn ApplicationBackupImportTokenGenerator>,
    pending: Mutex<PendingState>,
}

struct PendingImport {
    expires_at: Duration,
    logical_bytes: u64,
    prepared: PreparedApplicationBackup,
}

#[derive(Default)]
struct PendingState {
    active: BTreeMap<ApplicationBackupImportToken, PendingImport>,
    active_bytes: u64,
    retired: VecDeque<(ApplicationBackupImportToken, RetiredToken)>,
}

#[derive(Clone, Copy)]
enum RetiredToken {
    Expired,
    Consumed,
    Discarded,
}

impl ApplicationBackupImportPreparer {
    #[must_use]
    pub fn new() -> Self {
        Self::with_dependencies(
            ApplicationBackupArchiveLimits::default(),
            DEFAULT_APPLICATION_BACKUP_PENDING_TTL,
            DEFAULT_APPLICATION_BACKUP_PENDING_CAPACITY,
            DEFAULT_APPLICATION_BACKUP_PENDING_BYTES,
            Arc::new(SystemApplicationBackupImportClock),
            Arc::new(RandomApplicationBackupImportTokenGenerator),
        )
        .expect("default application backup pending limits are valid")
    }

    pub fn with_dependencies(
        limits: ApplicationBackupArchiveLimits,
        ttl: Duration,
        capacity: usize,
        max_pending_bytes: u64,
        clock: Arc<dyn ApplicationBackupImportClock>,
        tokens: Arc<dyn ApplicationBackupImportTokenGenerator>,
    ) -> AppResult<Self> {
        if ttl.is_zero() || capacity == 0 || capacity > 256 || max_pending_bytes == 0 {
            return Err(AppError::new(
                "APPLICATION_BACKUP_IMPORT_LIMITS_INVALID",
                "应用备份待确认容量或有效期配置无效。",
            ));
        }
        Ok(Self {
            limits,
            ttl,
            capacity,
            max_pending_bytes,
            clock,
            tokens,
            pending: Mutex::new(PendingState::default()),
        })
    }

    /// Removes and returns a prepared candidate exactly once. R07d will consume
    /// this boundary before entering its authoritative commit transaction.
    pub fn take(
        &self,
        token: ApplicationBackupImportToken,
    ) -> AppResult<PreparedApplicationBackup> {
        let now = self.clock.now();
        let mut state = self.pending.lock();
        if let Some(pending) = state.active.remove(&token) {
            state.active_bytes = state.active_bytes.saturating_sub(pending.logical_bytes);
            if pending.expires_at <= now {
                remember_retired(&mut state, token, RetiredToken::Expired, self.capacity);
                return Err(token_error(RetiredToken::Expired));
            }
            remember_retired(&mut state, token, RetiredToken::Consumed, self.capacity);
            return Ok(pending.prepared);
        }
        match state
            .retired
            .iter()
            .find(|(candidate, _)| *candidate == token)
        {
            Some((_, reason)) => Err(token_error(*reason)),
            None => Err(AppError::new(
                "APPLICATION_BACKUP_IMPORT_TOKEN_INVALID",
                "应用备份确认令牌无效。",
            )),
        }
    }

    fn discard_candidate(&self, token: ApplicationBackupImportToken) -> AppResult<()> {
        let now = self.clock.now();
        let mut state = self.pending.lock();
        retire_expired(&mut state, now, self.capacity);
        let pending = state.active.remove(&token).ok_or_else(|| {
            state
                .retired
                .iter()
                .find(|(candidate, _)| *candidate == token)
                .map_or_else(
                    || {
                        AppError::new(
                            "APPLICATION_BACKUP_IMPORT_TOKEN_INVALID",
                            "应用备份确认令牌无效。",
                        )
                    },
                    |(_, reason)| token_error(*reason),
                )
        })?;
        state.active_bytes = state.active_bytes.saturating_sub(pending.logical_bytes);
        remember_retired(&mut state, token, RetiredToken::Discarded, self.capacity);
        Ok(())
    }

    fn retain_candidate(
        &self,
        prepared: PreparedApplicationBackup,
    ) -> AppResult<(ApplicationBackupImportToken, Duration)> {
        let now = self.clock.now();
        let expires_at = now.checked_add(self.ttl).ok_or_else(|| {
            AppError::new(
                "APPLICATION_BACKUP_IMPORT_LIMITS_INVALID",
                "应用备份待确认有效期超出支持范围。",
            )
        })?;
        let mut state = self.pending.lock();
        retire_expired(&mut state, now, self.capacity);
        let logical_bytes = prepared.candidate.logical_bytes()?;
        if state.active.len() >= self.capacity
            || logical_bytes > self.max_pending_bytes.saturating_sub(state.active_bytes)
        {
            return Err(AppError::new(
                "APPLICATION_BACKUP_IMPORT_CAPACITY",
                "待确认的应用备份数量已达上限。",
            ));
        }
        let token = self.tokens.generate();
        if state.active.contains_key(&token)
            || state
                .retired
                .iter()
                .any(|(candidate, _)| *candidate == token)
        {
            return Err(AppError::new(
                "APPLICATION_BACKUP_IMPORT_TOKEN_COLLISION",
                "无法安全生成应用备份确认令牌。",
            ));
        }
        state.active.insert(
            token,
            PendingImport {
                expires_at,
                logical_bytes,
                prepared,
            },
        );
        state.active_bytes = state.active_bytes.saturating_add(logical_bytes);
        Ok((token, self.ttl))
    }
}

impl Default for ApplicationBackupImportPreparer {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ApplicationBackupImportPreparer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApplicationBackupImportPreparer")
            .field("ttl", &self.ttl)
            .field("capacity", &self.capacity)
            .field("pending_count", &self.pending.lock().active.len())
            .field("pending_logical_bytes", &self.pending.lock().active_bytes)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl ApplicationBackupImportPreparePort for ApplicationBackupImportPreparer {
    async fn read(&self, bytes: Vec<u8>) -> AppResult<ApplicationBackupImportCandidate> {
        let limits = self.limits.clone();
        tokio::task::spawn_blocking(move || reconstruct(&bytes, &limits))
            .await
            .map_err(|_| import_error())?
    }

    async fn retain(
        &self,
        prepared: PreparedApplicationBackup,
    ) -> AppResult<(ApplicationBackupImportToken, Duration)> {
        self.retain_candidate(prepared)
    }

    async fn discard(&self, token: ApplicationBackupImportToken) -> AppResult<()> {
        self.discard_candidate(token)
    }

    async fn take(
        &self,
        token: ApplicationBackupImportToken,
    ) -> AppResult<PreparedApplicationBackup> {
        ApplicationBackupImportPreparer::take(self, token)
    }
}

fn reconstruct(
    bytes: &[u8],
    limits: &ApplicationBackupArchiveLimits,
) -> AppResult<ApplicationBackupImportCandidate> {
    let archive =
        ApplicationBackupArchive::read_with_limits(bytes, limits).map_err(|_| import_error())?;
    let document = archive.document;
    let mut packages = Vec::with_capacity(document.protocol_packages.len());
    for package in document.protocol_packages {
        let prefix = format!(
            "protocol-packages/{}/{}/",
            package.package.id.as_str(),
            package.package.version.as_str()
        );
        let mut files = Vec::with_capacity(package.files.len());
        for path in package.files {
            let relative = path
                .as_str()
                .strip_prefix(&prefix)
                .ok_or_else(import_error)?;
            let contents = archive.files.get(&path).ok_or_else(import_error)?;
            files.push(PortableProtocolPackageFile {
                path: relative.to_owned(),
                contents_base64: STANDARD.encode(contents),
            });
        }
        packages.push(PortableApplicationProtocolPackage {
            package: package.package,
            files,
            enabled: package.enabled,
        });
    }
    let mut materials = Vec::with_capacity(document.portable_materials.len());
    for material in document.portable_materials {
        let contents = archive.files.get(&material.path).ok_or_else(import_error)?;
        materials.push(PortableCertificateMaterial {
            reference_id: material.reference_id,
            label: material.label,
            kind: material.kind,
            material_base64: STANDARD.encode(contents),
            material_sha256: portable_material_sha256(contents),
            password: material.password,
        });
    }
    Ok(ApplicationBackupImportCandidate {
        selected_workspace_id: document.application.selected_workspace_id,
        workspaces: document.application.workspaces,
        settings: document.application.settings,
        protocol_packages: packages,
        certificate_materials: materials,
        migration_report: MigrationReport::unchanged(
            MigrationSourceKind::ApplicationConfigurationDocument,
            APPLICATION_CONFIGURATION_FORMAT_VERSION,
        ),
    })
}

fn retire_expired(state: &mut PendingState, now: Duration, capacity: usize) {
    let expired = state
        .active
        .iter()
        .filter_map(|(token, pending)| (pending.expires_at <= now).then_some(*token))
        .collect::<Vec<_>>();
    for token in expired {
        if let Some(pending) = state.active.remove(&token) {
            state.active_bytes = state.active_bytes.saturating_sub(pending.logical_bytes);
        }
        remember_retired(state, token, RetiredToken::Expired, capacity);
    }
}

fn remember_retired(
    state: &mut PendingState,
    token: ApplicationBackupImportToken,
    reason: RetiredToken,
    capacity: usize,
) {
    state.retired.push_back((token, reason));
    while state.retired.len() > capacity {
        state.retired.pop_front();
    }
}

fn token_error(reason: RetiredToken) -> AppError {
    match reason {
        RetiredToken::Expired => AppError::new(
            "APPLICATION_BACKUP_IMPORT_TOKEN_EXPIRED",
            "应用备份确认令牌已过期。",
        ),
        RetiredToken::Consumed => AppError::new(
            "APPLICATION_BACKUP_IMPORT_TOKEN_CONSUMED",
            "应用备份确认令牌已使用。",
        ),
        RetiredToken::Discarded => AppError::new(
            "APPLICATION_BACKUP_IMPORT_TOKEN_DISCARDED",
            "应用备份确认令牌已取消。",
        ),
    }
}

fn import_error() -> AppError {
    AppError::new(
        "APPLICATION_BACKUP_IMPORT_INVALID",
        "应用备份 ZIP 无效或不完整。",
    )
}
