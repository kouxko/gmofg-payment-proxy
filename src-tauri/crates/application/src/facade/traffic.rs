//! 抓包、会话和断点用例。
//!
//! 查询规范化、敏感导出确认、断点校验和运行周期检查全部留在 Rust，使桌面界面和未来
//! 终端界面不会产生不同行为。

use chrono::Utc;

use super::{
    Application,
    validation::{ensure_valid, normalized_optional},
};
use crate::{
    AppError, AppResult, BreakpointDecision, BreakpointDetailViewModel, BreakpointDraft,
    BreakpointId, BreakpointSummaryViewModel, BreakpointValidationViewModel,
    CaptureDetailViewModel, CapturePageViewModel, CaptureQuery, OperationResultViewModel,
    ProxyState, RuntimeEpoch, SessionDetailViewModel, SessionId, SessionPageViewModel,
    SessionQuery, UiEventPayload,
};

impl Application {
    pub async fn capture_query(&self, mut query: CaptureQuery) -> AppResult<CapturePageViewModel> {
        query.keyword = normalized_optional(query.keyword);
        query.terminal_ip = normalized_optional(query.terminal_ip);
        query.result = normalized_optional(query.result);
        query.page = query.page.normalized();
        self.capture.query(query).await
    }

    pub async fn capture_get_detail(
        &self,
        session_id: SessionId,
        runtime_epoch: RuntimeEpoch,
    ) -> AppResult<CaptureDetailViewModel> {
        self.capture.get_detail(session_id, runtime_epoch).await
    }

    pub async fn capture_clear_view(&self, current_cursor: u64) -> AppResult<u64> {
        self.capture.clear_view(current_cursor).await
    }

    pub async fn session_query(&self, mut query: SessionQuery) -> AppResult<SessionPageViewModel> {
        query.keyword = normalized_optional(query.keyword);
        query.terminal_ip = normalized_optional(query.terminal_ip);
        query.result = normalized_optional(query.result);
        query.page = query.page.normalized();
        self.sessions.query(query).await
    }

    pub async fn session_get(&self, session_id: SessionId) -> AppResult<SessionDetailViewModel> {
        self.sessions.get(session_id).await
    }

    pub async fn session_export(
        &self,
        session_id: SessionId,
        sensitive_data_confirmed: bool,
    ) -> AppResult<OperationResultViewModel> {
        if !sensitive_data_confirmed {
            return Err(AppError::new(
                "EXPORT_CONFIRMATION_REQUIRED",
                "导出文件包含原始敏感数据，请确认后再导出。",
            ));
        }
        let session = self.sessions.get(session_id).await?;
        self.file_export
            .export_session(session, sensitive_data_confirmed)
            .await
    }

    pub async fn session_clear(&self, confirmed: bool) -> AppResult<OperationResultViewModel> {
        if !confirmed {
            return Err(AppError::new(
                "CONFIRMATION_REQUIRED",
                "清空已完成会话需要确认。",
            ));
        }
        let count = self.sessions.clear_completed().await?;
        Ok(OperationResultViewModel::success(format!(
            "已清空 {count} 个已完成会话，待处理断点未受影响。"
        )))
    }

    pub fn breakpoint_query(
        &self,
        runtime_epoch: Option<RuntimeEpoch>,
    ) -> Vec<BreakpointSummaryViewModel> {
        self.breakpoints.query(runtime_epoch)
    }

    pub fn breakpoint_get(
        &self,
        breakpoint_id: BreakpointId,
        runtime_epoch: RuntimeEpoch,
    ) -> AppResult<BreakpointDetailViewModel> {
        self.breakpoints.get(breakpoint_id, runtime_epoch)
    }

    pub fn breakpoint_format_json(&self, draft: BreakpointDraft) -> AppResult<BreakpointDraft> {
        self.breakpoint_validation.format_json(draft)
    }

    pub fn breakpoint_restore_original(
        &self,
        breakpoint_id: BreakpointId,
        runtime_epoch: RuntimeEpoch,
    ) -> AppResult<BreakpointDraft> {
        let detail = self.breakpoints.get(breakpoint_id, runtime_epoch)?;
        self.breakpoint_validation.restore_original(&detail)
    }

    pub fn breakpoint_validate(
        &self,
        draft: &BreakpointDraft,
        runtime_epoch: RuntimeEpoch,
    ) -> AppResult<BreakpointValidationViewModel> {
        let detail = self.breakpoints.get(draft.breakpoint_id, runtime_epoch)?;
        self.breakpoint_validation.validate(&detail, draft)
    }

    pub async fn breakpoint_resolve(
        &self,
        runtime_epoch: RuntimeEpoch,
        mut decision: BreakpointDecision,
    ) -> AppResult<BreakpointSummaryViewModel> {
        let status = self.proxy.status().await?;
        if status.state != ProxyState::Running || status.runtime_epoch != Some(runtime_epoch) {
            return Err(AppError::new(
                "BREAKPOINT_PROXY_STOPPED",
                "Proxy 未在对应运行周期中运行，不能处理断点。",
            )
            .epoch(runtime_epoch));
        }
        let detail = self
            .breakpoints
            .get(decision.breakpoint_id, runtime_epoch)?;
        if matches!(
            decision.kind,
            crate::BreakpointDecisionKind::ForwardModified
                | crate::BreakpointDecisionKind::MockResponse
        ) {
            let draft = BreakpointDraft {
                breakpoint_id: decision.breakpoint_id,
                expected_revision: decision.expected_revision,
                message: decision.message.clone().ok_or_else(|| {
                    AppError::field(
                        "CONFIG_INVALID",
                        "该断点处理方式必须提供报文。",
                        std::collections::BTreeMap::from([(
                            "message".into(),
                            vec!["该操作必须提供报文。".into()],
                        )]),
                    )
                })?,
            };
            decision.message = Some(self.breakpoint_validation.format_json(draft)?.message);
        }
        let validation = self
            .breakpoint_validation
            .validate_decision(&detail, &decision)?;
        ensure_valid("CONFIG_INVALID", "断点决策校验失败。", &validation)?;
        let summary = self.breakpoints.resolve(runtime_epoch, decision)?;
        self.events.publish(
            Some(runtime_epoch),
            Utc::now(),
            Some(summary.breakpoint_id.to_string()),
            Some(summary.revision),
            UiEventPayload::BreakpointResolved(summary.clone()),
        );
        Ok(summary)
    }
}
