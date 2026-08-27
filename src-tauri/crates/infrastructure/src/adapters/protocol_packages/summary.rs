//! 无源码协议包摘要、安装/恢复结果及其 `SQLite` header 投影。

use chrono::{DateTime, Utc};
use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_protocol_scripting::ProtocolPackageKind;

use crate::sqlite::protocol_packages::{
    StoredProtocolPackageHeader, StoredProtocolPackageValidation,
};

/// 无源码的持久化校验状态。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolPackageValidationStatus {
    /// 最近一次导入或缓存恢复完整通过。
    Valid,
    /// 数据库文件集合无法再通过路径、声明或 Rhai 编译校验。
    Invalid {
        /// 稳定机器码；不包含脚本内容、原始路径或第三方错误文本。
        code: String,
    },
}

/// 协议包列表和后续 Application 用例使用的无源码记录。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolPackageSummary {
    /// 不可变的应用级 ID 与精确 `SemVer`。
    pub package: ProtocolPackageRef,
    /// Manifest 中受长度和控制字符门禁保护的展示名称。
    pub name: String,
    /// 导入时已经确认受当前 Host 支持的 API 主版本。
    pub host_api: u32,
    /// 从严格 Manifest 推断的数据平面类型。
    pub kind: ProtocolPackageKind,
    /// 应用级启用位；新安装记录固定为 `false`。
    pub enabled: bool,
    /// 最近一次完整编译或缓存恢复结果。
    pub validation: ProtocolPackageValidationStatus,
    /// 首次安装时间；幂等重入不会改写。
    pub installed_at: DateTime<Utc>,
}

/// 幂等导入结果；相同身份与完全相同文件集合会复用现有记录。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolPackageInstallOutcome {
    Installed(ProtocolPackageSummary),
    Reused(ProtocolPackageSummary),
}

/// 启动时单个包的缓存恢复失败。
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolPackageRecoveryFailure {
    /// 无法恢复的精确版本。
    pub package: ProtocolPackageRef,
    /// 路径、声明或脚本阶段产生的稳定脱敏机器码。
    pub code: String,
}

/// 启动缓存恢复报告；一个坏包不会阻止其他独立版本恢复。
#[cfg(test)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProtocolPackageRecoveryReport {
    /// 已重新编译并进入进程缓存的版本。
    pub loaded: Vec<ProtocolPackageRef>,
    /// 已持久化标记为 Invalid 且没有进入缓存的版本。
    pub failed: Vec<ProtocolPackageRecoveryFailure>,
}

pub(super) fn summary_from_header(header: StoredProtocolPackageHeader) -> ProtocolPackageSummary {
    let validation = match header.validation {
        StoredProtocolPackageValidation::Valid => ProtocolPackageValidationStatus::Valid,
        StoredProtocolPackageValidation::Invalid(code) => {
            ProtocolPackageValidationStatus::Invalid { code }
        }
    };
    ProtocolPackageSummary {
        package: header.package,
        name: header.name,
        host_api: header.host_api,
        kind: header.kind,
        enabled: header.enabled,
        validation,
        installed_at: header.installed_at,
    }
}
