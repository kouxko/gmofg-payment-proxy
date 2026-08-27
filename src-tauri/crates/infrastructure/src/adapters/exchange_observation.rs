//! Exchange UI tracing 的连接级有界内存仓储。
//!
//! 仓储只接受已解析的强类型事件；tracing 字段解析属于桌面进程 Layer。所有写入均为
//! fail-open：容量不足会淘汰最旧连接或丢弃当前事件，并留下显式淘汰标记。

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use intercept_proxy_application::{
    CapacityLedger, ExchangeObservationEvent, ExchangeObservationPage, ExchangeObservationQuery,
    ExchangeObservationQueryPort, ExchangeObservationRecord, RuntimeEpoch,
};
use parking_lot::Mutex;

#[derive(Debug, Default)]
struct ObservationState {
    records: VecDeque<ExchangeObservationRecord>,
    logical_bytes: u64,
    evicted_by_workspace: BTreeMap<intercept_proxy_domain::WorkspaceId, u64>,
}

#[derive(Debug)]
struct Reclamation {
    indices: Vec<usize>,
    replacement_bytes: u64,
}

/// Producer-side counters are atomic because tracing callbacks must never acquire the Store lock.
#[derive(Debug, Default)]
pub struct ExchangeObservationCounters {
    dropped_events: AtomicU64,
    ignored_events: AtomicU64,
}

impl ExchangeObservationCounters {
    pub fn note_dropped(&self) {
        self.dropped_events.fetch_add(1, Ordering::Relaxed);
    }

    pub fn note_ignored(&self) {
        self.ignored_events.fetch_add(1, Ordering::Relaxed);
    }

    #[must_use]
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn ignored_events(&self) -> u64 {
        self.ignored_events.load(Ordering::Relaxed)
    }
}

/// UI command 与 MCP backend 必须共享同一个 `Arc` 实例。
#[derive(Debug)]
pub struct ExchangeObservationStore {
    capacity: Arc<CapacityLedger>,
    state: Mutex<ObservationState>,
    counters: Arc<ExchangeObservationCounters>,
}

impl ExchangeObservationStore {
    #[must_use]
    pub fn new(capacity: Arc<CapacityLedger>) -> Self {
        Self {
            capacity,
            state: Mutex::new(ObservationState::default()),
            counters: Arc::new(ExchangeObservationCounters::default()),
        }
    }

    /// Returns the lock-free counters shared with the tracing producer.
    #[must_use]
    pub fn counters(&self) -> Arc<ExchangeObservationCounters> {
        Arc::clone(&self.counters)
    }

    /// `opened` 是唯一创建连接记录的事件；缺失元数据由 Layer 忽略，不在这里猜测。
    pub fn open(&self, record: ExchangeObservationRecord) -> bool {
        let bytes = record.logical_bytes();
        let mut state = self.state.lock();
        if state
            .records
            .iter()
            .any(|item| item.exchange_id == record.exchange_id)
        {
            self.counters.note_ignored();
            return false;
        }
        let Some(reclamation) = self.reserve_replacement(&state, bytes, None) else {
            self.counters.note_ignored();
            return false;
        };
        apply_reclamation(&mut state, &reclamation);
        state.logical_bytes = reclamation.replacement_bytes;
        state.records.push_back(record);
        true
    }

    /// 事件按产生顺序追加，并返回已存在记录的运行期归属供调用方发布刷新事件。
    ///
    /// 找不到 `opened` 时 fail-open 忽略，禁止从当前事件猜测连接元数据。返回值使
    /// `EventHub` 发布不必要求每条 tracing 事件重复携带 `runtime_epoch`，同时避免写入
    /// 成功后再次加锁查询造成清空竞态。
    pub fn append(
        &self,
        exchange_id: &str,
        protocol: intercept_proxy_application::ExchangeProtocol,
        observed_runtime_epoch: Option<RuntimeEpoch>,
        event: ExchangeObservationEvent,
    ) -> Option<RuntimeEpoch> {
        let mut state = self.state.lock();
        let Some(index) = state
            .records
            .iter()
            .position(|record| record.exchange_id == exchange_id)
        else {
            self.counters.note_ignored();
            return None;
        };
        if state.records[index].protocol != protocol
            || observed_runtime_epoch
                .is_some_and(|epoch| epoch != state.records[index].runtime_epoch)
        {
            self.counters.note_ignored();
            return None;
        }
        let runtime_epoch = state.records[index].runtime_epoch;
        let before = state.records[index].logical_bytes();
        state.records[index].events.push(event);
        let after = state.records[index].logical_bytes();
        let added = after.saturating_sub(before);
        if let Some(reclamation) = self.reserve_replacement(&state, added, Some(exchange_id)) {
            apply_reclamation(&mut state, &reclamation);
            state.logical_bytes = reclamation.replacement_bytes;
            return Some(runtime_epoch);
        }

        // 当前单条事件无法容纳：恢复原状态，并在记录上显式标记证据淘汰。
        if let Some(record) = state
            .records
            .iter_mut()
            .find(|record| record.exchange_id == exchange_id)
        {
            record.events.pop();
            record.evidence_evicted = true;
        }
        self.counters.note_ignored();
        None
    }

    #[must_use]
    pub fn query(&self, query: &ExchangeObservationQuery) -> ExchangeObservationPage {
        let page = query.page.normalized();
        let state = self.state.lock();
        let matching = state
            .records
            .iter()
            .filter(|record| record.workspace_id == query.workspace_id)
            .filter(|record| {
                query
                    .listener_id
                    .is_none_or(|listener| listener == record.listener_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let total = matching.len() as u64;
        let start = (page.page.saturating_sub(1) as usize)
            .saturating_mul(page.page_size as usize)
            .min(matching.len());
        let end = start
            .saturating_add(page.page_size as usize)
            .min(matching.len());
        ExchangeObservationPage {
            rows: matching[start..end].to_vec(),
            page: page.page,
            page_size: page.page_size,
            total,
            evicted_records: state
                .evicted_by_workspace
                .get(&query.workspace_id)
                .copied()
                .unwrap_or_default(),
            dropped_events: self.counters.dropped_events(),
            ignored_events: self.counters.ignored_events(),
        }
    }

    #[must_use]
    pub fn get(&self, exchange_id: &str) -> Option<ExchangeObservationRecord> {
        self.state
            .lock()
            .records
            .iter()
            .find(|record| record.exchange_id == exchange_id)
            .cloned()
    }

    pub fn clear_workspace(&self, workspace_id: intercept_proxy_domain::WorkspaceId) -> usize {
        let mut state = self.state.lock();
        let mut removed_bytes = 0_u64;
        let before = state.records.len();
        state.records.retain(|record| {
            if record.workspace_id == workspace_id {
                removed_bytes = removed_bytes.saturating_add(record.logical_bytes());
                false
            } else {
                true
            }
        });
        state.logical_bytes = state.logical_bytes.saturating_sub(removed_bytes);
        state.evicted_by_workspace.remove(&workspace_id);
        self.capacity.release_capture_bytes(removed_bytes);
        before.saturating_sub(state.records.len())
    }

    #[must_use]
    pub fn ignored_events(&self) -> u64 {
        self.counters.ignored_events()
    }

    #[must_use]
    pub fn dropped_events(&self) -> u64 {
        self.counters.dropped_events()
    }

    /// Consumer 无法解析完整 primitive fields 时记录一个原子计数，不保存格式化原文。
    pub fn note_ignored_event(&self) {
        self.counters.note_ignored();
    }

    fn reserve_replacement(
        &self,
        state: &ObservationState,
        required: u64,
        protected_exchange_id: Option<&str>,
    ) -> Option<Reclamation> {
        let current = state.logical_bytes;
        let mut replacement = current.saturating_add(required);
        if self
            .capacity
            .try_replace_capture_bytes(current, replacement)
        {
            return Some(Reclamation {
                indices: Vec::new(),
                replacement_bytes: replacement,
            });
        }

        let mut indices = Vec::new();
        let mut removed_bytes = 0_u64;
        for (index, record) in state.records.iter().enumerate() {
            if protected_exchange_id == Some(record.exchange_id.as_str()) {
                continue;
            }
            indices.push(index);
            removed_bytes = removed_bytes.saturating_add(record.logical_bytes());
            replacement = current
                .saturating_sub(removed_bytes)
                .saturating_add(required);
            if self
                .capacity
                .try_replace_capture_bytes(current, replacement)
            {
                return Some(Reclamation {
                    indices,
                    replacement_bytes: replacement,
                });
            }
        }
        None
    }
}

impl ExchangeObservationQueryPort for ExchangeObservationStore {
    fn query(&self, query: &ExchangeObservationQuery) -> ExchangeObservationPage {
        Self::query(self, query)
    }

    fn get(&self, exchange_id: &str) -> Option<ExchangeObservationRecord> {
        Self::get(self, exchange_id)
    }
}

fn apply_reclamation(state: &mut ObservationState, reclamation: &Reclamation) {
    for index in reclamation.indices.iter().rev().copied() {
        let removed = state.records.remove(index).expect("planned record index");
        let evicted = state
            .evicted_by_workspace
            .entry(removed.workspace_id)
            .or_default();
        *evicted = evicted.saturating_add(1);
    }
}

impl Drop for ExchangeObservationStore {
    fn drop(&mut self) {
        self.capacity
            .release_capture_bytes(self.state.get_mut().logical_bytes);
    }
}

#[cfg(test)]
#[path = "exchange_observation/tests.rs"]
mod tests;
