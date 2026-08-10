use chrono::Utc;

use super::Application;
use crate::{
    DiagnosticLogEntryViewModel, DiagnosticLogPageViewModel, DiagnosticLogQuery, UiEventPayload,
};

impl Application {
    /// 供桌面、ADB、Companion 和网络编排步骤记录脱敏诊断结果。
    pub fn diagnostic_log_record(&self, entry: DiagnosticLogEntryViewModel) {
        let entry = entry.sanitized();
        self.events.publish(
            None,
            Utc::now(),
            entry
                .listener_id
                .clone()
                .or_else(|| entry.device_serial.clone()),
            None,
            UiEventPayload::DiagnosticLogAdded(entry),
        );
    }

    /// 查询由 Rust 保留和筛选的统一日志；前端不得自行读取系统日志或持久化日志。
    #[must_use]
    pub fn diagnostic_log_query(&self, query: &DiagnosticLogQuery) -> DiagnosticLogPageViewModel {
        let current_cursor = self.events.current_cursor();
        let keyword = query
            .keyword
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_lowercase);
        let mut rows = self.events.diagnostic_log_snapshot();
        rows.retain(|row| {
            query
                .after_event_id
                .is_none_or(|cursor| row.event_id > cursor)
                && keyword.as_ref().is_none_or(|needle| {
                    [
                        row.summary.as_str(),
                        row.detail.as_deref().unwrap_or_default(),
                        row.stage_text.as_str(),
                        row.device_serial.as_deref().unwrap_or_default(),
                        row.listener_id.as_deref().unwrap_or_default(),
                        row.profile_id.as_deref().unwrap_or_default(),
                    ]
                    .iter()
                    .any(|value| value.to_lowercase().contains(needle))
                })
        });
        let retained_count = rows.len();
        let limit = usize::from(query.limit.clamp(1, 500));
        rows.reverse();
        rows.truncate(limit);
        DiagnosticLogPageViewModel {
            truncated: retained_count > rows.len(),
            rows,
            current_cursor,
            retained_count,
            empty_message: "暂无诊断日志。执行设备连接、路由、代理入口或 TLS 操作后将在此显示。"
                .to_owned(),
        }
    }
}
