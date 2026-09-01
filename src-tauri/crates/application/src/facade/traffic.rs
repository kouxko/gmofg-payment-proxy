//! 抓包和会话用例。
//!
//! 查询规范化、敏感导出确认和运行周期隔离全部留在 Rust，使桌面界面和未来
//! 终端界面不会产生不同行为。

use super::{Application, validation::normalized_optional};
use crate::{
    AppError, AppResult, CaptureDetailViewModel, CapturePageViewModel, CaptureQuery,
    OperationResultViewModel, RuntimeEpoch, SessionDetailViewModel, SessionId,
    SessionListViewModel, SessionQuery,
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
            "已清空 {count} 个已完成会话。"
        )))
    }
}
