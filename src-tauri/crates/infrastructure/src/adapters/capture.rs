//! 抓包会话的应用查询适配器。
//!
//! 会话正文只存在于受容量约束的运行时存储中，本模块负责分页、筛选和详情映射；找不到
//! 或已被淘汰都显式返回，不把“空结果”伪装成成功命中。

use std::{
    cmp::Ordering,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering as AtomicOrdering},
    },
};

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, CaptureDetailViewModel, CapturePageViewModel, CaptureQuery,
    CaptureRepositoryPort, CaptureRowViewModel, CaptureSort, InMemorySessionStore, RuntimeEpoch,
    SessionId, SessionQueryPort, SortDirection,
};
use parking_lot::RwLock;

#[derive(Debug)]
pub struct CaptureRepositoryAdapter {
    rows: RwLock<Vec<CaptureRowViewModel>>,
    sessions: Arc<InMemorySessionStore>,
    view_floor: AtomicU64,
    latest_runtime_epoch: RwLock<Option<RuntimeEpoch>>,
}

impl CaptureRepositoryAdapter {
    const MAX_ROWS: usize = 4_096;

    #[must_use]
    pub fn new(sessions: Arc<InMemorySessionStore>) -> Self {
        Self {
            rows: RwLock::new(Vec::new()),
            sessions,
            view_floor: AtomicU64::new(0),
            latest_runtime_epoch: RwLock::new(None),
        }
    }

    pub fn push_for_epoch(&self, row: CaptureRowViewModel, runtime_epoch: RuntimeEpoch) {
        debug_assert_eq!(row.runtime_epoch, runtime_epoch);
        *self.latest_runtime_epoch.write() = Some(runtime_epoch);
        let mut rows = self.rows.write();
        if let Some(existing) = rows
            .iter_mut()
            .find(|existing| existing.event_id == row.event_id)
        {
            *existing = row;
        } else {
            rows.push(row);
            if rows.len() > Self::MAX_ROWS
                && let Some((oldest, _)) = rows
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, candidate)| candidate.event_id)
            {
                rows.remove(oldest);
            }
        }
    }

    #[cfg(test)]
    pub fn push(&self, row: CaptureRowViewModel) {
        let runtime_epoch = row.runtime_epoch;
        self.push_for_epoch(row, runtime_epoch);
    }
}

#[async_trait]
impl CaptureRepositoryPort for CaptureRepositoryAdapter {
    async fn query(&self, query: CaptureQuery) -> AppResult<CapturePageViewModel> {
        let page = query.page.normalized();
        let keyword = normalized(query.keyword.as_deref());
        let terminal = normalized(query.terminal_ip.as_deref());
        let result = normalized(query.result.as_deref());
        let floor = self.view_floor.load(AtomicOrdering::Acquire);
        let rows_guard = self.rows.read();
        let oldest_event_id = rows_guard
            .iter()
            .filter(|row| row.event_id > floor)
            .map(|row| row.event_id)
            .min();
        let latest_event_id = rows_guard.iter().map(|row| row.event_id).max().unwrap_or(0);
        let snapshot_required = query.after_event_id.is_some_and(|cursor| {
            cursor < latest_event_id
                && oldest_event_id.is_some_and(|oldest| cursor.saturating_add(1) < oldest)
        });
        let mut rows = rows_guard
            .iter()
            .filter(|row| row.event_id > floor)
            .filter(|row| {
                query
                    .after_event_id
                    .is_none_or(|after_event_id| row.event_id > after_event_id)
            })
            .filter(|row| {
                keyword.as_ref().is_none_or(|needle| {
                    contains(&row.session_id.to_string(), needle)
                        || contains(&row.target, needle)
                        || contains(&row.method, needle)
                })
            })
            .filter(|row| {
                terminal
                    .as_ref()
                    .is_none_or(|needle| contains(&row.terminal_ip, needle))
            })
            .filter(|row| {
                query
                    .channel
                    .as_ref()
                    .is_none_or(|channel| &row.channel == channel)
            })
            .filter(|row| query.stage.is_none_or(|stage| row.stage == stage))
            .filter(|row| {
                result
                    .as_ref()
                    .is_none_or(|needle| contains(&row.result, needle))
            })
            .filter(|row| {
                query
                    .rule_id
                    .is_none_or(|rule_id| row.matched_rule_ids.contains(&rule_id))
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            let order =
                compare(left, right, query.sort).then_with(|| left.event_id.cmp(&right.event_id));
            match query.direction {
                SortDirection::Asc => order,
                SortDirection::Desc => order.reverse(),
            }
        });
        let total = rows.len();
        let start = page.page.saturating_sub(1).saturating_mul(page.page_size) as usize;
        let rows = rows
            .into_iter()
            .skip(start)
            .take(page.page_size as usize)
            .collect();
        Ok(CapturePageViewModel {
            rows,
            total,
            page: page.page,
            page_size: page.page_size,
            total_pages: total
                .div_ceil(page.page_size as usize)
                .try_into()
                .unwrap_or(u32::MAX),
            event_cursor: latest_event_id,
            oldest_event_id,
            runtime_epoch: *self.latest_runtime_epoch.read(),
            snapshot_required,
            empty_message: if total == 0 {
                "没有符合条件的抓包事件。".into()
            } else {
                String::new()
            },
        })
    }

    async fn get_detail(
        &self,
        session_id: SessionId,
        runtime_epoch: RuntimeEpoch,
    ) -> AppResult<CaptureDetailViewModel> {
        let session = self.sessions.get(session_id).await?;
        if runtime_epoch != session.runtime_epoch {
            return Err(
                AppError::new("SESSION_NOT_FOUND", "抓包详情不属于当前运行周期。")
                    .entity(session_id.to_string()),
            );
        }
        let request = session.request.ok_or_else(|| {
            AppError::new("SESSION_NOT_FOUND", "抓包请求详情尚未生成或已被淘汰。")
                .entity(session_id.to_string())
        })?;
        Ok(CaptureDetailViewModel {
            session_id,
            request_id: session.summary.request_id,
            terminal_ip: session.summary.terminal_ip,
            certificate_fingerprint_suffix: fingerprint_suffix(&session.certificate_fingerprint),
            upstream_host: session.upstream_host,
            request,
            response: session.response,
            tls_summary: format!(
                "{}；{}",
                session.app_to_proxy_tls, session.proxy_to_server_tls
            ),
            timings_ms: session.timings_ms,
            rule_trace: session.rule_trace,
            revision: session.summary.revision,
        })
    }

    async fn clear_view(&self, current_cursor: u64) -> AppResult<u64> {
        let latest = self
            .rows
            .read()
            .iter()
            .map(|row| row.event_id)
            .max()
            .unwrap_or(current_cursor);
        self.view_floor.store(latest, AtomicOrdering::Release);
        Ok(latest)
    }
}

fn fingerprint_suffix(fingerprint: &str) -> String {
    let compact = fingerprint.replace(':', "");
    let start = compact.len().saturating_sub(8);
    compact[start..].to_owned()
}

fn normalized(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase)
}

fn contains(value: &str, needle: &str) -> bool {
    value.to_lowercase().contains(needle)
}

fn compare(left: &CaptureRowViewModel, right: &CaptureRowViewModel, sort: CaptureSort) -> Ordering {
    match sort {
        CaptureSort::OccurredAt => left.occurred_at.cmp(&right.occurred_at),
        CaptureSort::TerminalIp => left.terminal_ip.cmp(&right.terminal_ip),
        CaptureSort::Duration => left.duration_ms.cmp(&right.duration_ms),
        CaptureSort::Size => left.size_bytes.cmp(&right.size_bytes),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use intercept_proxy_application::{
        CaptureSort, ChannelId, MessageStage, PageRequest, SortDirection, UiTone,
    };
    use uuid::Uuid;

    use super::*;

    fn test_adapter() -> CaptureRepositoryAdapter {
        CaptureRepositoryAdapter::new(Arc::new(InMemorySessionStore::default()))
    }

    fn row(event_id: u64, terminal: &str) -> CaptureRowViewModel {
        CaptureRowViewModel {
            event_id,
            runtime_epoch: Uuid::nil(),
            session_id: Uuid::from_u128(u128::from(event_id)),
            occurred_at: Utc
                .with_ymd_and_hms(
                    2026,
                    7,
                    28,
                    0,
                    0,
                    u32::try_from(event_id % 60).expect("time"),
                )
                .single()
                .expect("timestamp"),
            terminal_ip: terminal.into(),
            channel: ChannelId::new("alpha").unwrap(),
            channel_text: "交易".into(),
            stage: MessageStage::Request,
            stage_text: "请求".into(),
            method: "POST".into(),
            target: "/payment".into(),
            http_status: None,
            result: "成功".into(),
            ui_tone: UiTone::Positive,
            duration_ms: Some(event_id),
            matched_rule_ids: Vec::new(),
            size_bytes: event_id,
        }
    }

    // CAPTURE-003~006, TEST-EVENT
    #[tokio::test]
    async fn query_and_clear_are_deterministic_without_deleting_rows() {
        let adapter = test_adapter();
        adapter.push(row(2, "10.0.0.2"));
        adapter.push(row(1, "10.0.0.1"));
        let query = CaptureQuery {
            keyword: Some("payment".into()),
            terminal_ip: Some("10.0.0".into()),
            channel: Some(ChannelId::new("alpha").unwrap()),
            stage: Some(MessageStage::Request),
            result: Some("成功".into()),
            rule_id: None,
            after_event_id: None,
            sort: CaptureSort::OccurredAt,
            direction: SortDirection::Asc,
            page: PageRequest {
                page: 1,
                page_size: 1,
            },
        };
        let first = adapter.query(query.clone()).await.expect("query");
        assert_eq!(first.total, 2);
        assert_eq!(first.rows[0].event_id, 1);
        assert_eq!(adapter.clear_view(1).await.expect("clear"), 2);
        assert_eq!(adapter.query(query).await.expect("after clear").total, 0);
        assert_eq!(
            adapter.rows.read().len(),
            2,
            "clear only advances view cursor"
        );
    }

    #[tokio::test]
    async fn resume_cursor_is_epoch_scoped_and_reports_retention_gap() {
        let adapter = test_adapter();
        let epoch = Uuid::new_v4();
        for event_id in 1..=4_097 {
            let mut row = row(event_id, "10.0.0.1");
            row.runtime_epoch = epoch;
            adapter.push(row);
        }
        let page = adapter
            .query(CaptureQuery {
                keyword: None,
                terminal_ip: None,
                channel: None,
                stage: None,
                result: None,
                rule_id: None,
                after_event_id: Some(0),
                sort: CaptureSort::OccurredAt,
                direction: SortDirection::Asc,
                page: PageRequest {
                    page: 1,
                    page_size: 10,
                },
            })
            .await
            .expect("resume");
        assert_eq!(page.runtime_epoch, Some(epoch));
        assert_eq!(page.oldest_event_id, Some(2));
        assert_eq!(page.event_cursor, 4_097);
        assert!(page.snapshot_required);
        assert!(page.rows.iter().all(|row| row.runtime_epoch == epoch));
    }
}
