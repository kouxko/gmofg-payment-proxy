use std::{cmp::Ordering, collections::HashMap};

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::{
    AppError, AppResult, PageRequest, SessionDetailViewModel, SessionId, SessionPageViewModel,
    SessionQuery, SessionQueryPort, SessionRecord, SessionSort, SessionSummaryViewModel,
    SortDirection,
};

pub trait SessionStore: Send + Sync + std::fmt::Debug {
    fn upsert(&self, record: SessionRecord) -> AppResult<Vec<SessionId>>;
    fn get(&self, session_id: SessionId) -> AppResult<SessionDetailViewModel>;
    fn query(&self, query: &SessionQuery) -> SessionPageViewModel;
    fn clear_completed(&self) -> usize;
    fn logical_bytes(&self) -> u64;
    fn set_pending_ui_event_bytes(&self, bytes: u64) -> AppResult<Vec<SessionId>>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug)]
pub struct InMemorySessionStore {
    limits: RwLock<CapacityLimits>,
    state: RwLock<StoreState>,
}

#[derive(Debug, Clone, Copy)]
struct CapacityLimits {
    max_sessions: usize,
    max_bytes: u64,
}

#[derive(Debug, Default)]
struct StoreState {
    records: HashMap<SessionId, SessionRecord>,
    record_bytes: u64,
    pending_ui_event_bytes: u64,
}

impl InMemorySessionStore {
    pub const DEFAULT_MAX_SESSIONS: usize = 500;
    pub const DEFAULT_MAX_BYTES: u64 = 256 * 1024 * 1024;

    pub fn new(max_sessions: usize, max_bytes: u64) -> Self {
        Self {
            limits: RwLock::new(CapacityLimits {
                max_sessions: max_sessions.max(1),
                max_bytes: max_bytes.max(1),
            }),
            state: RwLock::new(StoreState::default()),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(Self::DEFAULT_MAX_SESSIONS, Self::DEFAULT_MAX_BYTES)
    }

    pub fn set_limits(&self, max_sessions: usize, max_bytes: u64) -> AppResult<Vec<SessionId>> {
        let candidate = CapacityLimits {
            max_sessions: max_sessions.max(1),
            max_bytes: max_bytes.max(1),
        };
        let mut limits = self.limits.write();
        let mut state = self.state.write();
        let evicted = eviction_plan(&state, candidate, None)?;
        apply_evictions(&mut state, &evicted);
        *limits = candidate;
        Ok(evicted)
    }

    pub fn get_record(&self, session_id: SessionId) -> AppResult<SessionRecord> {
        self.state
            .read()
            .records
            .get(&session_id)
            .cloned()
            .ok_or_else(|| {
                AppError::new("SESSION_NOT_FOUND", "会话不存在或已被容量策略淘汰。")
                    .entity(session_id.to_string())
            })
    }
}

impl Default for InMemorySessionStore {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl SessionStore for InMemorySessionStore {
    fn upsert(&self, record: SessionRecord) -> AppResult<Vec<SessionId>> {
        let id = record.id();
        let limits = *self.limits.read();
        let mut state = self.state.write();
        let previous = state.records.remove(&id);
        if let Some(old) = &previous {
            state.record_bytes = state.record_bytes.saturating_sub(old.logical_bytes());
        }
        state.record_bytes = state.record_bytes.saturating_add(record.logical_bytes());
        state.records.insert(id, record);

        let evicted = match eviction_plan(&state, limits, Some(id)) {
            Ok(evicted) => evicted,
            Err(error) => {
                if let Some(current) = state.records.remove(&id) {
                    state.record_bytes = state.record_bytes.saturating_sub(current.logical_bytes());
                }
                if let Some(previous) = previous {
                    state.record_bytes =
                        state.record_bytes.saturating_add(previous.logical_bytes());
                    state.records.insert(id, previous);
                }
                return Err(error);
            }
        };
        apply_evictions(&mut state, &evicted);
        Ok(evicted)
    }

    fn get(&self, session_id: SessionId) -> AppResult<SessionDetailViewModel> {
        self.state
            .read()
            .records
            .get(&session_id)
            .map(|record| record.detail.clone())
            .ok_or_else(|| {
                AppError::new("SESSION_NOT_FOUND", "会话不存在或已被容量策略淘汰。")
                    .entity(session_id.to_string())
            })
    }

    fn query(&self, query: &SessionQuery) -> SessionPageViewModel {
        let normalized_page = query.page.normalized();
        let keyword = normalize_filter(query.keyword.as_deref());
        let terminal_ip = normalize_filter(query.terminal_ip.as_deref());
        let result = normalize_filter(query.result.as_deref());

        let state = self.state.read();
        let mut items = state
            .records
            .values()
            .map(|record| &record.detail.summary)
            .filter(|summary| {
                keyword.as_ref().is_none_or(|needle| {
                    contains_case_insensitive(&summary.request_id, needle)
                        || contains_case_insensitive(&summary.target, needle)
                        || contains_case_insensitive(&summary.session_id.to_string(), needle)
                })
            })
            .filter(|summary| {
                terminal_ip
                    .as_ref()
                    .is_none_or(|needle| contains_case_insensitive(&summary.terminal_ip, needle))
            })
            .filter(|summary| {
                query
                    .channel
                    .as_ref()
                    .is_none_or(|channel| &summary.channel == channel)
            })
            .filter(|summary| {
                result
                    .as_ref()
                    .is_none_or(|needle| contains_case_insensitive(&summary.result, needle))
            })
            .filter(|summary| {
                query
                    .rule_id
                    .is_none_or(|rule_id| summary.matched_rule_ids.contains(&rule_id))
            })
            .filter(|summary| {
                query
                    .started_from
                    .is_none_or(|from| summary.started_at >= from)
            })
            .filter(|summary| query.started_to.is_none_or(|to| summary.started_at <= to))
            .cloned()
            .collect::<Vec<_>>();

        items.sort_by(|left, right| {
            let order = compare_sessions(left, right, query.sort)
                .then_with(|| left.session_id.cmp(&right.session_id));
            match query.direction {
                SortDirection::Asc => order,
                SortDirection::Desc => order.reverse(),
            }
        });

        let total = items.len();
        let start = (normalized_page.page.saturating_sub(1) as usize)
            .saturating_mul(normalized_page.page_size as usize);
        let items = items
            .into_iter()
            .skip(start)
            .take(normalized_page.page_size as usize)
            .collect();
        let total_pages = total
            .div_ceil(normalized_page.page_size as usize)
            .try_into()
            .unwrap_or(u32::MAX);

        SessionPageViewModel {
            items,
            total,
            page: normalized_page.page,
            page_size: normalized_page.page_size,
            total_pages,
            empty_message: if total == 0 {
                "没有符合条件的会话。".into()
            } else {
                String::new()
            },
        }
    }

    fn clear_completed(&self) -> usize {
        let mut state = self.state.write();
        let before = state.records.len();
        state.records.retain(|_, record| {
            record.detail.summary.completed_at.is_none() || record.is_pending()
        });
        state.record_bytes = state
            .records
            .values()
            .map(SessionRecord::logical_bytes)
            .sum();
        before - state.records.len()
    }

    fn logical_bytes(&self) -> u64 {
        let state = self.state.read();
        state
            .record_bytes
            .saturating_add(state.pending_ui_event_bytes)
    }

    fn set_pending_ui_event_bytes(&self, bytes: u64) -> AppResult<Vec<SessionId>> {
        let limits = *self.limits.read();
        let mut state = self.state.write();
        // UI event queues live outside this store and cannot be rolled back
        // here. Always commit their observed logical size so capacity
        // accounting remains truthful even when the combined limit is
        // exhausted. Record eviction itself remains all-or-nothing.
        state.pending_ui_event_bytes = bytes;
        let evicted = eviction_plan(&state, limits, None)?;
        apply_evictions(&mut state, &evicted);
        Ok(evicted)
    }

    fn len(&self) -> usize {
        self.state.read().records.len()
    }
}

#[async_trait]
impl SessionQueryPort for InMemorySessionStore {
    async fn query(&self, query: SessionQuery) -> AppResult<SessionPageViewModel> {
        Ok(SessionStore::query(self, &query))
    }

    async fn get(&self, session_id: SessionId) -> AppResult<SessionDetailViewModel> {
        SessionStore::get(self, session_id)
    }

    async fn clear_completed(&self) -> AppResult<usize> {
        Ok(SessionStore::clear_completed(self))
    }
}

fn normalize_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase)
}

fn contains_case_insensitive(value: &str, needle: &str) -> bool {
    value.to_lowercase().contains(needle)
}

fn eviction_order(left: &SessionRecord, right: &SessionRecord) -> Ordering {
    left.detail
        .summary
        .completed_at
        .cmp(&right.detail.summary.completed_at)
        .then_with(|| left.id().cmp(&right.id()))
}

fn eviction_plan(
    state: &StoreState,
    limits: CapacityLimits,
    protected: Option<SessionId>,
) -> AppResult<Vec<SessionId>> {
    let mut candidates = state
        .records
        .values()
        .filter(|record| {
            record.detail.summary.completed_at.is_some()
                && !record.is_pending()
                && Some(record.id()) != protected
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| eviction_order(left, right));

    let mut record_count = state.records.len();
    let mut logical_bytes = state
        .record_bytes
        .saturating_add(state.pending_ui_event_bytes);
    let mut evicted = Vec::new();
    let mut candidate_index = 0;
    while record_count > limits.max_sessions || logical_bytes > limits.max_bytes {
        let Some(candidate) = candidates.get(candidate_index) else {
            return Err(AppError::new(
                "RESOURCE_EXHAUSTED",
                "会话或内存容量已耗尽，且没有可淘汰的已完成会话。",
            )
            .retryable("请先处理待处理断点或清空已完成会话。"));
        };
        candidate_index += 1;
        record_count = record_count.saturating_sub(1);
        logical_bytes = logical_bytes.saturating_sub(candidate.logical_bytes());
        evicted.push(candidate.id());
    }
    Ok(evicted)
}

fn apply_evictions(state: &mut StoreState, evicted: &[SessionId]) {
    for session_id in evicted {
        if let Some(record) = state.records.remove(session_id) {
            state.record_bytes = state.record_bytes.saturating_sub(record.logical_bytes());
        }
    }
}

fn compare_sessions(
    left: &SessionSummaryViewModel,
    right: &SessionSummaryViewModel,
    sort: SessionSort,
) -> Ordering {
    match sort {
        SessionSort::StartedAt => left.started_at.cmp(&right.started_at),
        SessionSort::TerminalIp => left.terminal_ip.cmp(&right.terminal_ip),
        SessionSort::Duration => left.duration_ms.cmp(&right.duration_ms),
        SessionSort::RequestSize => left.request_size_bytes.cmp(&right.request_size_bytes),
        SessionSort::ResponseSize => left.response_size_bytes.cmp(&right.response_size_bytes),
    }
}

#[allow(dead_code)]
fn _page_request_is_public_contract(_: PageRequest) {}
