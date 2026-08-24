//! `ExternalPackageClient` 与单连接 Actor 之间的有界命令协议。

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use super::super::error::{ExternalPackageConnectionError, ExternalPackageFatalProtocolError};

pub(super) enum DataCommand {
    Call(CallCommand),
    Cancel(String),
}

pub(super) enum ControlCommand {
    Close,
    ProtocolFatal(ExternalPackageFatalProtocolError),
}

pub(super) struct CallCommand {
    pub(super) request_id: String,
    pub(super) method: String,
    pub(super) params: Value,
    pub(super) response_limit: usize,
    pub(super) response: oneshot::Sender<Result<Value, ExternalPackageConnectionError>>,
}

pub(super) struct PendingCall {
    pub(super) method: String,
    pub(super) response_limit: usize,
    pub(super) response: oneshot::Sender<Result<Value, ExternalPackageConnectionError>>,
}

/// RPC future 被放弃时，向 Actor 发送本地取消标记并释放关联状态。
pub(super) struct CancellationOnDrop {
    request_id: Option<String>,
    commands: mpsc::Sender<DataCommand>,
}

impl CancellationOnDrop {
    pub(super) fn new(request_id: String, commands: mpsc::Sender<DataCommand>) -> Self {
        Self {
            request_id: Some(request_id),
            commands,
        }
    }

    pub(super) fn complete(&mut self) {
        self.request_id = None;
    }
}

impl Drop for CancellationOnDrop {
    fn drop(&mut self) {
        if let Some(request_id) = self.request_id.take() {
            let _ = self.commands.try_send(DataCommand::Cancel(request_id));
        }
    }
}
