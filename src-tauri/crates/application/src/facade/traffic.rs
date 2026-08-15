//! 抓包、会话和断点用例。
//!
//! 查询规范化、敏感导出确认、断点校验和运行周期隔离全部留在 Rust，使桌面界面和未来
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
    RuntimeEpoch, SessionDetailViewModel, SessionId, SessionListViewModel, SessionQuery,
    SocketCaptureDetailViewModel, SocketCaptureId, SocketCapturePageViewModel, SocketCaptureQuery,
    UiEventPayload, WorkspaceId,
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

    /// 查询 Socket Frame/LocalExchange；该路径不会触碰旧 HTTP capture DTO。
    pub async fn socket_capture_query(
        &self,
        mut query: SocketCaptureQuery,
    ) -> AppResult<SocketCapturePageViewModel> {
        query.page = query.page.normalized();
        self.capture.query_socket(query).await
    }

    pub async fn socket_capture_get_detail(
        &self,
        capture_id: SocketCaptureId,
    ) -> AppResult<SocketCaptureDetailViewModel> {
        self.capture.get_socket_detail(capture_id).await
    }

    pub async fn socket_capture_clear(
        &self,
        workspace_id: WorkspaceId,
        confirmed: bool,
    ) -> AppResult<OperationResultViewModel> {
        if !confirmed {
            return Err(AppError::new(
                "CONFIRMATION_REQUIRED",
                "清空 Socket 抓包需要确认。",
            ));
        }
        let _gate = self.mutation_gate.lock().await;
        let selected = self
            .workspaces
            .list()
            .await?
            .into_iter()
            .filter(|workspace| workspace.selected)
            .collect::<Vec<_>>();
        let [selected] = selected.as_slice() else {
            return Err(AppError::new(
                "WORKSPACE_SELECTION_REQUIRED",
                "必须唯一选择一个 Workspace，才能清空 Socket 抓包。",
            )
            .entity(workspace_id.to_string()));
        };
        if selected.id != workspace_id {
            return Err(AppError::new(
                "WORKSPACE_SELECTION_CHANGED",
                "Workspace 已切换，请重新确认后再清空 Socket 抓包。",
            )
            .entity(workspace_id.to_string()));
        }
        let count = self.capture.clear_socket_completed(workspace_id).await?;
        Ok(OperationResultViewModel::success(format!(
            "已清空 {count} 条 Socket 抓包记录。"
        )))
    }

    pub async fn session_query(&self, mut query: SessionQuery) -> AppResult<SessionListViewModel> {
        query.keyword = normalized_optional(query.keyword);
        query.terminal_ip = normalized_optional(query.terminal_ip);
        query.result = normalized_optional(query.result);
        self.sessions.query(query).await
    }

    pub async fn session_get(&self, session_id: SessionId) -> AppResult<SessionDetailViewModel> {
        self.sessions.get(session_id).await
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

    pub fn breakpoint_resolve(
        &self,
        runtime_epoch: RuntimeEpoch,
        mut decision: BreakpointDecision,
    ) -> AppResult<BreakpointSummaryViewModel> {
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
            decision.message = Some(self.breakpoint_validation.normalize(draft)?.message);
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
