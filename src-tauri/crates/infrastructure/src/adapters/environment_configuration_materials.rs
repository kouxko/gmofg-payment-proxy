#[cfg(test)]
use std::sync::Weak;
use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, EnvironmentPreparedMaterialCapability, EnvironmentPreparedMaterialKind,
    EnvironmentPreparedMaterialVisitor, EnvironmentPreparedMaterials,
    EnvironmentProtectedMaterialPreparePort, MaterialAlias, StagedProtectedMaterialHandle,
};
use parking_lot::Mutex;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::SecretProtector;

/// Temporary protector output. Drop is explicit so early returns and panics erase the buffer.
struct PreparedProtectedMaterialHandle {
    protected: Zeroizing<Vec<u8>>,
}

pub(crate) struct PreparedMaterialRecord {
    pub(crate) alias: MaterialAlias,
    pub(crate) fingerprint: [u8; 32],
    pub(crate) protected_payload: Zeroizing<Vec<u8>>,
}

pub(crate) struct PreparedMaterialBatch {
    pub(crate) certificates: Vec<PreparedMaterialRecord>,
    pub(crate) secrets: Vec<PreparedMaterialRecord>,
}

struct PreparedMaterialArenaState<K> {
    records: HashMap<K, PreparedMaterialRecord>,
}

impl<K> Default for PreparedMaterialArenaState<K> {
    fn default() -> Self {
        Self {
            records: HashMap::new(),
        }
    }
}

struct PreparedMaterialArenaInner<K> {
    state: Mutex<PreparedMaterialArenaState<K>>,
}

#[derive(Clone)]
pub(crate) struct PreparedMaterialArena<K = PreparedMaterialReservation> {
    inner: Arc<PreparedMaterialArenaInner<K>>,
}

impl<K> Default for PreparedMaterialArena<K> {
    fn default() -> Self {
        Self {
            inner: Arc::new(PreparedMaterialArenaInner {
                state: Mutex::new(PreparedMaterialArenaState::default()),
            }),
        }
    }
}

impl<K> std::fmt::Debug for PreparedMaterialArena<K> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedMaterialArena")
            .finish_non_exhaustive()
    }
}

impl<K> PreparedMaterialArena<K>
where
    K: Clone + Eq + std::hash::Hash + Send + 'static,
{
    #[cfg(test)]
    fn stage_record(&self, key: &K, record: PreparedMaterialRecord) -> Box<dyn FnOnce() + Send> {
        self.inner.state.lock().records.insert(key.clone(), record);
        let weak = Arc::downgrade(&self.inner);
        let key = key.clone();
        Box::new(move || discard_from_weak(&weak, &key))
    }

    #[cfg(test)]
    pub(crate) fn stage_for_test(&self, key: &K, payload: Vec<u8>) -> Box<dyn FnOnce() + Send> {
        self.stage_record(
            key,
            PreparedMaterialRecord {
                alias: MaterialAlias::parse("test").expect("test alias"),
                fingerprint: [0x38; 32],
                protected_payload: Zeroizing::new(payload),
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn consume_for_test(&self, key: &K) -> AppResult<()> {
        self.inner
            .state
            .lock()
            .records
            .remove(key)
            .map(|_| ())
            .ok_or_else(invalid_capability)
    }

    #[cfg(test)]
    pub(crate) fn discard_for_test(&self, key: &K) -> AppResult<()> {
        self.consume_for_test(key)
    }

    #[cfg(test)]
    pub(crate) fn retained_batch_count_for_test(&self) -> usize {
        self.inner.state.lock().records.len()
    }

    #[cfg(test)]
    pub(crate) fn all_retained_bytes_are_zero_for_test(&self) -> bool {
        self.inner
            .state
            .lock()
            .records
            .values()
            .all(|record| record.protected_payload.iter().all(|byte| *byte == 0))
    }
}

#[cfg(test)]
fn discard_from_weak<K>(arena: &Weak<PreparedMaterialArenaInner<K>>, key: &K)
where
    K: Eq + std::hash::Hash,
{
    if let Some(arena) = arena.upgrade() {
        arena.state.lock().records.remove(key);
    }
}

fn invalid_capability() -> AppError {
    AppError::new(
        "PREPARED_MATERIAL_CAPABILITY_INVALID",
        "受保护材料能力不属于当前提交适配器。",
    )
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) struct PreparedMaterialReservation(Uuid);

struct ArenaPreparedMaterialCapability {
    arena: Arc<PreparedMaterialArena>,
    reservation: Option<PreparedMaterialReservation>,
}

impl EnvironmentPreparedMaterialCapability for ArenaPreparedMaterialCapability {
    fn consume(
        mut self: Box<Self>,
        kind: EnvironmentPreparedMaterialKind,
        alias: MaterialAlias,
        visitor: &mut dyn EnvironmentPreparedMaterialVisitor,
    ) -> AppResult<()> {
        let reservation = self.reservation.take().ok_or_else(invalid_capability)?;
        let record = self
            .arena
            .inner
            .state
            .lock()
            .records
            .remove(&reservation)
            .ok_or_else(invalid_capability)?;
        if record.alias != alias {
            return Err(invalid_capability());
        }
        visitor.visit(kind, alias, record.fingerprint, record.protected_payload)
    }
}

impl Drop for ArenaPreparedMaterialCapability {
    fn drop(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            self.arena.inner.state.lock().records.remove(&reservation);
        }
    }
}

impl PreparedMaterialArena {
    fn stage_capability(
        self: &Arc<Self>,
        record: PreparedMaterialRecord,
    ) -> Box<dyn EnvironmentPreparedMaterialCapability> {
        let reservation = PreparedMaterialReservation(Uuid::new_v4());
        self.inner.state.lock().records.insert(reservation, record);
        Box::new(ArenaPreparedMaterialCapability {
            arena: Arc::clone(self),
            reservation: Some(reservation),
        })
    }
}

impl PreparedProtectedMaterialHandle {
    fn new(protected: Vec<u8>) -> Self {
        Self {
            protected: Zeroizing::new(protected),
        }
    }

    fn finish_cleanup(
        mut self,
        alias: MaterialAlias,
        fingerprint: [u8; 32],
    ) -> PreparedMaterialRecord {
        PreparedMaterialRecord {
            alias,
            fingerprint,
            protected_payload: std::mem::take(&mut self.protected),
        }
    }
}

impl Drop for PreparedProtectedMaterialHandle {
    fn drop(&mut self) {
        self.protected.zeroize();
    }
}

pub struct EnvironmentConfigurationMaterialPreparer {
    protector: Arc<dyn SecretProtector>,
    arena: Arc<PreparedMaterialArena>,
}

impl std::fmt::Debug for EnvironmentConfigurationMaterialPreparer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnvironmentConfigurationMaterialPreparer")
            .field("protector", &"<system secret protector>")
            .finish()
    }
}

impl EnvironmentConfigurationMaterialPreparer {
    pub(crate) fn new(
        protector: Arc<dyn SecretProtector>,
        arena: Arc<PreparedMaterialArena>,
    ) -> Self {
        Self { protector, arena }
    }
}

#[async_trait]
impl EnvironmentProtectedMaterialPreparePort for EnvironmentConfigurationMaterialPreparer {
    async fn prepare(
        &self,
        staged: StagedProtectedMaterialHandle,
    ) -> AppResult<EnvironmentPreparedMaterials> {
        staged.prepare_with(|plaintext, alias, fingerprint| {
            let protected = self
                .protector
                .protect(plaintext)
                .map_err(|_| stable_error("PROTECTED_MATERIAL_PREPARE_FAILED"))?;
            let record =
                PreparedProtectedMaterialHandle::new(protected).finish_cleanup(alias, fingerprint);
            Ok(self.arena.stage_capability(record))
        })
    }
}

fn stable_error(code: &'static str) -> AppError {
    AppError::new(code, "受保护材料准备失败。")
}
