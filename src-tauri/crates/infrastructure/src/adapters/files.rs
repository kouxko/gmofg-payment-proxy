//! 原生文件选择与受控导入/导出的应用端口实现。
//!
//! `WebView` 只提交用途，不直接获得任意文件系统能力；取消选择是正常结果，读取超限、格式
//! 错误和写入失败则保留为可诊断错误。

use std::{fmt, path::PathBuf};

use intercept_proxy_application::{AppResult, OperationResultViewModel, UiTone};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSelection {
    pub path: PathBuf,
    pub overwrite_confirmed: bool,
}

pub trait NativeFileDialog: fmt::Debug + Send + Sync {
    fn choose_open_file(&self, purpose: &str) -> AppResult<Option<PathBuf>>;
    fn choose_save_file(
        &self,
        purpose: &str,
        suggested_file_name: &str,
    ) -> AppResult<Option<FileSelection>>;
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
