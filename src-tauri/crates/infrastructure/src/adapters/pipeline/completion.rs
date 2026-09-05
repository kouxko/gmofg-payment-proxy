use super::{
    AppError, AppMessageStage, CapturePublication, ConnectionContext, Ordering, ProxyResult,
    RuntimePipelineAdapter, SessionRecord, SessionStore, UiEventPayload, Utc,
};
use intercept_proxy_traffic_observation::{
    CaptureRowFacts, SessionCompletionOutcome, project_capture_row, project_session_completion,
};

impl RuntimePipelineAdapter {
    pub(super) fn finish_session(&self, context: &ConnectionContext, result: &ProxyResult<()>) {
        let (session_id, live) = {
            let mut state = self.state.lock();
            let Some(session_id) = state
                .connection(context)
                .and_then(|connection| connection.session_id)
            else {
                return;
            };
            (session_id, state.live_sessions.remove(&session_id))
        };
        let Some(live) = live else {
            return;
        };
        let Ok(mut record) = self.sessions.get_record(session_id) else {
            return;
        };
        let now = Utc::now();
        let duration_ms = u64::try_from(
            now.signed_duration_since(live.started_at)
                .num_milliseconds()
                .max(0),
        )
        .unwrap_or(u64::MAX);
        project_session_completion(
            &mut record,
            now,
            duration_ms,
            match result {
                Ok(()) => SessionCompletionOutcome::Success,
                Err(error) => SessionCompletionOutcome::Failure {
                    code: error.code,
                    message: &error.message,
                },
            },
        );
        let summary = record.detail.summary.clone();
        if let Err(error) = self.sessions.upsert(record.clone()) {
            self.resource_exhausted(context, &error);
            return;
        }
        self.events.publish(
            Some(context.runtime_epoch),
            now,
            Some(summary.session_id.to_string()),
            Some(summary.revision),
            UiEventPayload::SessionUpdated(summary.clone()),
        );
        self.publish_capture(
            context,
            &record,
            CapturePublication {
                stage: AppMessageStage::Terminal,
                result: &summary.result,
                tone: summary.ui_tone,
                size_bytes: summary
                    .request_size_bytes
                    .saturating_add(summary.response_size_bytes),
            },
        );
    }

    pub(super) fn publish_capture(
        &self,
        context: &ConnectionContext,
        record: &SessionRecord,
        publication: CapturePublication<'_>,
    ) {
        let event_id = self
            .capture_cursor
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let summary = &record.detail.summary;
        let row = project_capture_row(
            record,
            CaptureRowFacts {
                event_id,
                runtime_epoch: context.runtime_epoch,
                occurred_at: Utc::now(),
                channel_text: self.channel_label(summary.channel.as_str()),
                stage: publication.stage,
                result: publication.result,
                ui_tone: publication.tone,
                size_bytes: publication.size_bytes,
            },
        );
        self.captures
            .push_for_epoch(row.clone(), context.runtime_epoch);
        let _ = self
            .events
            .push_capture(context.runtime_epoch, Utc::now(), row);
    }

    pub(super) fn resource_exhausted(&self, context: &ConnectionContext, error: &AppError) {
        {
            let mut state = self.state.lock();
            if let Some(metrics) = state.channel_metrics_mut(context) {
                metrics.error_count = metrics.error_count.saturating_add(1);
            }
        }
        self.events.publish(
            Some(context.runtime_epoch),
            Utc::now(),
            None,
            None,
            UiEventPayload::ResourceWarning {
                message: error.view_model.message.clone(),
            },
        );
        self.events.publish(
            Some(context.runtime_epoch),
            Utc::now(),
            error.view_model.entity_id.clone(),
            None,
            UiEventPayload::OperationFailed((*error.view_model).clone()),
        );
    }

    /// 上报已建立的上游 TLS/mTLS 证据无法写入会话的故障。
    ///
    /// 调用此方法时传输层握手已经成功。即使会话已消失，或容量账本拒绝扩大的记录，也要把
    /// 证据文本保留在操作失败事件中，确保诊断事实可观察，同时不触发 panic，也不伪装成写入
    /// 成功。
    pub(super) fn upstream_security_persistence_failed(
        &self,
        context: &ConnectionContext,
        entity_id: String,
        error: AppError,
        evidence_text: &str,
    ) {
        {
            let mut state = self.state.lock();
            if let Some(metrics) = state.channel_metrics_mut(context) {
                metrics.error_count = metrics.error_count.saturating_add(1);
            }
        }

        let mut failure = *error.view_model;
        failure.entity_id = Some(entity_id);
        failure.runtime_epoch = Some(context.runtime_epoch);
        failure.message = format!(
            "无法保存已建立的上游安全证据：{} 本次证据：{evidence_text}",
            failure.message
        );

        if failure.code == "RESOURCE_EXHAUSTED" {
            self.events.publish(
                Some(context.runtime_epoch),
                Utc::now(),
                failure.entity_id.clone(),
                None,
                UiEventPayload::ResourceWarning {
                    message: failure.message.clone(),
                },
            );
        }
        self.events.publish(
            Some(context.runtime_epoch),
            Utc::now(),
            failure.entity_id.clone(),
            None,
            UiEventPayload::OperationFailed(failure),
        );
    }
}
