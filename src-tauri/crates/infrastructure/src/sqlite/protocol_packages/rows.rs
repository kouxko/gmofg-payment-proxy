//! 协议包持久化行模型及严格解析。
//!
//! SQL 查询和事务留在父模块；本模块只负责把不可信的 `SQLite` 标量恢复为领域可用的
//! 持久化对象。任何损坏都会被显式标记或作为持久化错误返回。

use chrono::{DateTime, Utc};
use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use intercept_proxy_protocol_scripting::ProtocolPackageKind;
use uuid::Uuid;

use super::InfrastructureError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StoredProtocolPackageValidation {
    Valid,
    Invalid(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredProtocolPackageHeader {
    pub package: ProtocolPackageRef,
    pub name: String,
    pub host_api: u32,
    pub kind: ProtocolPackageKind,
    pub enabled: bool,
    pub validation: StoredProtocolPackageValidation,
    pub installed_at: DateTime<Utc>,
    /// 每次真正安装生成的新代际；幂等重入保留旧值，删除后重装必定变化。
    pub generation: Uuid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredProtocolPackage {
    pub header: StoredProtocolPackageHeader,
    pub files: StoredProtocolPackageFiles,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StoredProtocolPackageFiles {
    Valid(Vec<(String, Vec<u8>)>),
    /// `SQLite` 被外部破坏后可能超过正常导入上限；预检失败时不读取任何 BLOB。
    Rejected(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg(test)]
pub(crate) enum StoredProtocolPackageInstallOutcome {
    Installed(Uuid),
    Reused(Uuid),
    IdentityConflict,
}

pub(super) type HeaderRow = (
    String,
    String,
    String,
    i64,
    String,
    i64,
    String,
    Option<String>,
    String,
    String,
);

pub(super) fn read_header_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HeaderRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}

pub(super) fn parse_header(
    row: HeaderRow,
) -> Result<StoredProtocolPackageHeader, InfrastructureError> {
    let (id, version, name, host_api, kind, enabled, state, error_code, installed_at, generation) =
        row;
    let package = ProtocolPackageRef {
        id: ProtocolPackageId::new(id).map_err(|_| corrupt_protocol_package("package_id 无效"))?,
        version: ProtocolPackageVersion::new(version)
            .map_err(|_| corrupt_protocol_package("version 无效"))?,
    };
    let mut metadata_corrupt = false;
    let name = if valid_display_name(&name) {
        name
    } else {
        metadata_corrupt = true;
        "Invalid protocol package".to_owned()
    };
    let host_api = u32::try_from(host_api).unwrap_or_else(|_| {
        metadata_corrupt = true;
        0
    });
    let kind = match kind.as_str() {
        "http" => ProtocolPackageKind::Http,
        "socket" => ProtocolPackageKind::Socket,
        _ => {
            metadata_corrupt = true;
            ProtocolPackageKind::Http
        }
    };
    let enabled = match enabled {
        0 => false,
        1 => true,
        _ => {
            metadata_corrupt = true;
            false
        }
    };
    let validation = match (state.as_str(), error_code) {
        ("valid", None) => StoredProtocolPackageValidation::Valid,
        ("invalid", Some(code)) if valid_error_code(&code) => {
            StoredProtocolPackageValidation::Invalid(code)
        }
        _ => {
            metadata_corrupt = true;
            StoredProtocolPackageValidation::Invalid("PERSISTENCE_CORRUPT".to_owned())
        }
    };
    let installed_at = DateTime::parse_from_rfc3339(&installed_at).map_or_else(
        |_| {
            metadata_corrupt = true;
            DateTime::<Utc>::UNIX_EPOCH
        },
        |value| value.with_timezone(&Utc),
    );
    let generation = Uuid::parse_str(&generation).unwrap_or_else(|_| {
        metadata_corrupt = true;
        Uuid::nil()
    });
    let validation = if metadata_corrupt {
        StoredProtocolPackageValidation::Invalid("PERSISTENCE_CORRUPT".to_owned())
    } else {
        validation
    };
    Ok(StoredProtocolPackageHeader {
        package,
        name,
        host_api,
        kind,
        enabled,
        validation,
        installed_at,
        generation,
    })
}

fn valid_error_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_display_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 128 && !name.chars().any(char::is_control)
}

fn corrupt_protocol_package(message: impl Into<String>) -> InfrastructureError {
    InfrastructureError::PersistenceCorrupt {
        entity: "protocol_package",
        message: message.into(),
    }
}
