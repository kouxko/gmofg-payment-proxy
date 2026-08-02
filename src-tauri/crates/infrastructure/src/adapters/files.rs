//! 原生文件选择与受控导入/导出的应用端口实现。
//!
//! `WebView` 只提交用途，不直接获得任意文件系统能力；取消选择是正常结果，读取超限、格式
//! 错误和写入失败则保留为可诊断错误。

use std::{fmt, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, FileExportPort, OperationResultViewModel, SessionDetailViewModel, UiTone,
};
use zeroize::Zeroizing;

use crate::AtomicFileExporter;

use super::common::{infra, json_error};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSelection {
    pub path: PathBuf,
    pub overwrite_confirmed: bool,
}

pub trait NativeFileDialog: fmt::Debug + Send + Sync {
    fn choose_open_file(&self, purpose: &str) -> AppResult<Option<PathBuf>>;
    fn choose_save_file(&self, purpose: &str) -> AppResult<Option<FileSelection>>;
}

#[derive(Debug)]
pub struct FileExportAdapter {
    dialog: Arc<dyn NativeFileDialog>,
    exporter: AtomicFileExporter,
}

impl FileExportAdapter {
    #[must_use]
    pub fn new(dialog: Arc<dyn NativeFileDialog>) -> Self {
        Self {
            dialog,
            exporter: AtomicFileExporter,
        }
    }
}

#[async_trait]
impl FileExportPort for FileExportAdapter {
    async fn export_session(
        &self,
        session: SessionDetailViewModel,
        sensitive_data_confirmed: bool,
    ) -> AppResult<OperationResultViewModel> {
        if !sensitive_data_confirmed {
            return Err(AppError::new(
                "EXPORT_CONFIRMATION_REQUIRED",
                "导出文件包含原始敏感数据，请确认后再导出。",
            ));
        }
        let Some(selection) = self.dialog.choose_save_file("session_json")? else {
            return Ok(cancelled("已取消会话导出。"));
        };
        let bytes = Zeroizing::new(
            serde_json::to_vec_pretty(&session)
                .map_err(|error| json_error("会话导出序列化失败", error))?,
        );
        let outcome = infra(self.exporter.write(
            &selection.path,
            &bytes,
            selection.overwrite_confirmed,
        ))?;
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: format!("会话已导出，共写入 {} 字节。", outcome.bytes_written),
            ui_tone: UiTone::Positive,
            entity_id: Some(session.summary.session_id.to_string()),
            revision: Some(session.summary.revision),
            requires_restart: false,
        })
    }
}

pub(crate) fn cancelled(message: &str) -> OperationResultViewModel {
    OperationResultViewModel {
        success: false,
        cancelled: true,
        message: message.into(),
        ui_tone: UiTone::Neutral,
        entity_id: None,
        revision: None,
        requires_restart: false,
    }
}
