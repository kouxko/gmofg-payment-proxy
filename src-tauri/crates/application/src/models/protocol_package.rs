//! 协议包生命周期的无源码应用模型。
//!
//! 这些类型只描述 Manifest 元数据、启用状态、校验结果和引用者。脚本源码、AST、ZIP
//! 原始字节与本机路径都不能进入 Application 返回模型，后续任何 UI/CLI 适配器只能基于
//! 这些经过收敛的字段展示协议包。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{ListenerId, ListenerRuntimeState, ProtocolPackageId, ProtocolPackageRef, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
/// 最近一次持久化校验的无源码结果。
pub enum ProtocolPackageValidationViewModel {
    Valid,
    Invalid { code: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 一个不可变协议包版本的轻量摘要。
pub struct ProtocolPackageVersionViewModel {
    pub package: ProtocolPackageRef,
    pub name: String,
    pub host_api: u32,
    pub enabled: bool,
    pub validation: ProtocolPackageValidationViewModel,
    pub installed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 列表按协议包 ID 聚合，但每个版本仍保留自己的名称、状态和精确身份。
pub struct ProtocolPackageGroupViewModel {
    pub id: ProtocolPackageId,
    pub versions: Vec<ProtocolPackageVersionViewModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 一个已保存 Listener 对精确协议包版本的引用。
///
/// `runtime_state` 由 Rust 合并运行时状态；没有活动运行记录时固定为 `Stopped`。删除只看
/// 引用是否存在，停用则仅允许所有引用都已确认停止。
pub struct ProtocolPackageUsageViewModel {
    pub workspace_id: WorkspaceId,
    pub workspace_name: String,
    pub listener_id: ListenerId,
    pub listener_name: String,
    pub listener_enabled: bool,
    pub runtime_state: ListenerRuntimeState,
}

impl ProtocolPackageUsageViewModel {
    #[must_use]
    pub fn blocks_disable(&self) -> bool {
        self.runtime_state != ListenerRuntimeState::Stopped
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 精确版本详情；不包含脚本、文件列表、安装路径或编译器内部对象。
pub struct ProtocolPackageDetailViewModel {
    pub version: ProtocolPackageVersionViewModel,
    pub usages: Vec<ProtocolPackageUsageViewModel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Compiler port 完整重验后返回的收据。
///
/// Application 会再次比较精确身份和 Host API，防止端口误把另一个版本的成功结果用于
/// 当前启用操作。该内部模型不需要序列化给展示层。
pub struct ProtocolPackageCompilationReceipt {
    pub package: ProtocolPackageRef,
    pub host_api: u32,
    pub compatible: bool,
}
