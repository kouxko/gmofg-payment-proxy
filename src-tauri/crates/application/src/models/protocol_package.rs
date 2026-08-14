//! 协议包生命周期的无源码应用模型。
//!
//! 这些类型只描述 Manifest 元数据、启用状态、校验结果和引用者。脚本源码、AST、ZIP
//! 原始字节与本机路径都不能进入 Application 返回模型，后续任何 UI/CLI 适配器只能基于
//! 这些经过收敛的字段展示协议包。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

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
    /// 最新已安装版本声明的名称，作为分组行的稳定展示名称。
    pub name: String,
    pub versions: Vec<ProtocolPackageVersionViewModel>,
    /// 全部精确版本被已保存 Listener 引用的总次数。
    pub reference_count: usize,
    /// 其中运行态不为 Stopped 的引用次数。
    pub active_reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 一个已保存 Listener 对精确协议包版本的引用。
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

#[derive(Debug, Clone, PartialEq, Eq)]
/// Usage 端口一次扫描生成的精确版本计数；仅在 Application 内汇总，不直接进入 IPC。
pub struct ProtocolPackageUsageCount {
    pub package: ProtocolPackageRef,
    pub reference_count: usize,
    pub active_reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 精确版本详情；不包含脚本、文件列表、安装路径或编译器内部对象。
pub struct ProtocolPackageDetailViewModel {
    pub version: ProtocolPackageVersionViewModel,
    pub capabilities: ProtocolPackageCapabilitiesViewModel,
    pub schema: ProtocolPackageSchemaViewModel,
    pub usages: Vec<ProtocolPackageUsageViewModel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 一个方向在 Manifest 中声明并通过编译校验的能力。
/// Frame 与 Decode 在 Host API v1 中是必需入口，仍显式返回给 UI，避免前端根据 API
/// 版本自行推断。Encode 是可选入口；为 `false` 时对应开关必须保持关闭。
pub struct ProtocolPackageDirectionCapabilitiesViewModel {
    pub frame: bool,
    pub decode: bool,
    pub encode: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 协议包两个方向与公共 Display 的完整能力投影。
pub struct ProtocolPackageCapabilitiesViewModel {
    pub upstream: ProtocolPackageDirectionCapabilitiesViewModel,
    pub downstream: ProtocolPackageDirectionCapabilitiesViewModel,
    pub display: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 一个按作者声明顺序返回的 Schema 字段。
pub struct ProtocolPackageSchemaFieldViewModel {
    pub name: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: ProtocolPackageSchemaFieldTypeViewModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
/// Host API v1 Schema 可向 UI 和规则目录公开的四种字段类型。
pub enum ProtocolPackageSchemaFieldTypeViewModel {
    String,
    Int,
    Bool,
    Blob,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 协议包提前声明的 Document Schema；不包含 Schema 文件路径或原始 TOML。
pub struct ProtocolPackageSchemaViewModel {
    pub id: String,
    pub version: u32,
    pub title: String,
    pub fields: Vec<ProtocolPackageSchemaFieldViewModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 基础设施从已编译包生成的安全描述，用于详情与导入结果。
pub struct ProtocolPackageDescriptionViewModel {
    pub capabilities: ProtocolPackageCapabilitiesViewModel,
    pub schema: ProtocolPackageSchemaViewModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
/// 原生 ZIP 导入是新安装，还是相同身份和内容的幂等复用。
pub enum ProtocolPackageImportOutcomeViewModel {
    Installed,
    Reused,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
/// 一次已验证但未安装的 pending import 随机令牌。
pub struct ProtocolPackageImportToken(Uuid);

impl ProtocolPackageImportToken {
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// ZIP 已完整校验、尚未安装时返回给确认 Dialog 的无源码预览。
pub struct ProtocolPackageImportPreviewViewModel {
    pub token: ProtocolPackageImportToken,
    pub package: ProtocolPackageRef,
    pub name: String,
    pub host_api: u32,
    pub capabilities: ProtocolPackageCapabilitiesViewModel,
    pub schema: ProtocolPackageSchemaViewModel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 原生文件选择和完整校验成功后的无源码导入结果。
/// 用户取消由命令返回 `None` 表示；任何 ZIP、Manifest、Schema 或 Rhai 错误均作为
/// 稳定 `AppError` 返回，不会构造该类型。
pub struct ProtocolPackageImportViewModel {
    pub outcome: ProtocolPackageImportOutcomeViewModel,
    pub version: ProtocolPackageVersionViewModel,
    pub capabilities: ProtocolPackageCapabilitiesViewModel,
    pub schema: ProtocolPackageSchemaViewModel,
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
