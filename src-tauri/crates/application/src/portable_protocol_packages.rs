//! v4 可移植文档中的协议包文件载荷。
//!
//! 这里只验证 JSON wire 自身可以安全、确定地交给恢复层：身份唯一、路径规范、Base64
//! 规范且资源有界。Manifest、Schema、Rhai 和完整的跨平台路径冲突检查仍由
//! infrastructure 调用协议脚本恢复/编译器完成，Application 不复制第二套解析器。

use std::{cmp::Ordering, collections::HashSet};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use intercept_proxy_domain::{
    ListenerDataPlane, ProtocolPackageRef, ProxyWorkspace, SocketPayloadProcessing,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{AppError, AppResult};

pub const MAX_PORTABLE_PROTOCOL_PACKAGES: usize = 256;
pub const MAX_PORTABLE_PACKAGE_FILES: usize = 512;
pub const MAX_PORTABLE_PACKAGE_FILE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PORTABLE_PACKAGE_TOTAL_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PORTABLE_PACKAGE_PATH_BYTES: usize = 256;
pub const MAX_PORTABLE_PACKAGE_PATH_DEPTH: usize = 32;

/// 一个协议包文件。`contents_base64` 使用标准、有填充的 RFC 4648 Base64。
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PortableProtocolPackageFile {
    pub path: String,
    pub contents_base64: String,
}

/// Workspace 文档内嵌的精确协议包；导入时默认停用，因此不携带 `enabled`。
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PortableProtocolPackage {
    pub package: ProtocolPackageRef,
    /// 必须按 `path` 严格升序排列，保证相同包产生稳定 wire。
    pub files: Vec<PortableProtocolPackageFile>,
}

/// 完整应用配置内嵌的已安装协议包及其启用状态。
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PortableApplicationProtocolPackage {
    pub package: ProtocolPackageRef,
    /// 必须按 `path` 严格升序排列，保证相同包产生稳定 wire。
    pub files: Vec<PortableProtocolPackageFile>,
    pub enabled: bool,
}

pub(crate) trait PortablePackageEntry {
    fn package(&self) -> &ProtocolPackageRef;
    fn files(&self) -> &[PortableProtocolPackageFile];
}

impl PortablePackageEntry for PortableProtocolPackage {
    fn package(&self) -> &ProtocolPackageRef {
        &self.package
    }

    fn files(&self) -> &[PortableProtocolPackageFile] {
        &self.files
    }
}

impl PortablePackageEntry for PortableApplicationProtocolPackage {
    fn package(&self) -> &ProtocolPackageRef {
        &self.package
    }

    fn files(&self) -> &[PortableProtocolPackageFile] {
        &self.files
    }
}

pub(crate) fn validate_portable_packages<T: PortablePackageEntry>(packages: &[T]) -> AppResult<()> {
    if packages.len() > MAX_PORTABLE_PROTOCOL_PACKAGES {
        return Err(invalid("协议包数量超过 256 个安全上限。"));
    }

    let mut previous_identity: Option<&ProtocolPackageRef> = None;
    for package in packages {
        if previous_identity.is_some_and(|previous| {
            compare_package_identity(previous, package.package()) != Ordering::Less
        }) {
            return Err(invalid(
                "协议包必须按 id 和 SemVer 严格升序排列且不能重复。",
            ));
        }
        previous_identity = Some(package.package());
        validate_files(package.files())?;
    }
    Ok(())
}

fn compare_package_identity(left: &ProtocolPackageRef, right: &ProtocolPackageRef) -> Ordering {
    left.id.as_str().cmp(right.id.as_str()).then_with(|| {
        left.version
            .semantic_cmp(&right.version)
            // SemVer precedence忽略 build metadata；相同 precedence 时再按原文排序，
            // 使合法但精确身份不同的 `+build` 版本仍有唯一 wire 顺序。
            .then_with(|| left.version.as_str().cmp(right.version.as_str()))
    })
}

fn validate_files(files: &[PortableProtocolPackageFile]) -> AppResult<()> {
    if files.is_empty() || files.len() > MAX_PORTABLE_PACKAGE_FILES {
        return Err(invalid("协议包文件数量必须为 1 到 512。"));
    }

    let mut previous_path: Option<&str> = None;
    let mut total_bytes = 0_usize;
    for file in files {
        validate_basic_path(&file.path)?;
        if previous_path.is_some_and(|previous| previous >= file.path.as_str()) {
            return Err(invalid("协议包文件必须按路径严格升序排列且不能重复。"));
        }
        previous_path = Some(&file.path);

        let decoded = STANDARD
            .decode(&file.contents_base64)
            .map_err(|_| invalid("协议包文件内容不是有效的标准 Base64。"))?;
        if STANDARD.encode(&decoded) != file.contents_base64 {
            return Err(invalid("协议包文件内容必须使用规范的标准 Base64 编码。"));
        }
        if decoded.len() > MAX_PORTABLE_PACKAGE_FILE_BYTES {
            return Err(invalid("协议包单文件超过 8 MiB 安全上限。"));
        }
        total_bytes = total_bytes
            .checked_add(decoded.len())
            .ok_or_else(|| invalid("协议包文件累计大小溢出。"))?;
        if total_bytes > MAX_PORTABLE_PACKAGE_TOTAL_BYTES {
            return Err(invalid("单个协议包文件累计超过 32 MiB 安全上限。"));
        }
    }
    Ok(())
}

fn validate_basic_path(path: &str) -> AppResult<()> {
    let valid_segments = path
        .split('/')
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    let valid_characters = path
        .chars()
        .all(|character| !character.is_control() && character != '\\' && character != ':');
    if path.is_empty()
        || path.len() > MAX_PORTABLE_PACKAGE_PATH_BYTES
        || path.starts_with('/')
        || path.split('/').count() > MAX_PORTABLE_PACKAGE_PATH_DEPTH
        || !valid_segments
        || !valid_characters
    {
        return Err(invalid("协议包文件路径不是安全的相对路径。"));
    }
    Ok(())
}

pub(crate) fn validate_configuration_package_references<T: PortablePackageEntry>(
    workspaces: &[ProxyWorkspace],
    packages: &[T],
    require_references: bool,
) -> AppResult<()> {
    validate_portable_packages(packages)?;
    if !require_references {
        return Ok(());
    }
    let embedded = packages
        .iter()
        .map(|package| package.package().clone())
        .collect::<HashSet<_>>();
    if referenced_packages(workspaces).is_subset(&embedded) {
        Ok(())
    } else {
        Err(invalid(
            "完整配置中的全部 Scripted Listener 和 Socket 规则都必须引用内嵌协议包。",
        ))
    }
}

fn referenced_packages(workspaces: &[ProxyWorkspace]) -> HashSet<ProtocolPackageRef> {
    let listeners = workspaces.iter().flat_map(|workspace| {
        workspace.listeners.iter().filter_map(|listener| {
            let ListenerDataPlane::Socket(socket) = &listener.data_plane else {
                return None;
            };
            let SocketPayloadProcessing::Scripted(scripted) = &socket.processing else {
                return None;
            };
            Some(scripted.package.clone())
        })
    });
    let rules = workspaces.iter().flat_map(|workspace| {
        workspace
            .socket_rules
            .iter()
            .map(|rule| rule.package().clone())
    });
    listeners.chain(rules).collect()
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::new("PORTABLE_PROTOCOL_PACKAGE_INVALID", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use intercept_proxy_domain::{
        DirectionProcessingOptions, ListenerDataPlane, ProtocolPackageId, ProtocolPackageVersion,
        ProxyListener, ScriptedSocketProcessing, SocketEndpoint, SocketPayloadProcessing,
        SocketRelaySecurity, SocketRelaySettings,
    };

    fn package() -> PortableProtocolPackage {
        PortableProtocolPackage {
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new("example").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            files: vec![PortableProtocolPackageFile {
                path: "manifest.toml".into(),
                contents_base64: STANDARD.encode(b"manifest"),
            }],
        }
    }

    fn scripted_workspace(package: ProtocolPackageRef) -> ProxyWorkspace {
        let listener = ProxyListener {
            data_plane: ListenerDataPlane::Socket(SocketRelaySettings::relay(
                SocketEndpoint {
                    host: "upstream.example.test".into(),
                    port: 9000,
                },
                SocketRelaySecurity::Transparent,
                1_000,
                SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                    package,
                    upstream: DirectionProcessingOptions {
                        decode_enabled: true,
                        encode_enabled: false,
                    },
                    downstream: DirectionProcessingOptions {
                        decode_enabled: true,
                        encode_enabled: false,
                    },
                }),
            )),
            ..ProxyListener::default()
        };
        ProxyWorkspace {
            listeners: vec![listener],
            ..ProxyWorkspace::default()
        }
    }

    #[test]
    fn package_files_require_canonical_base64_and_stable_unique_paths() {
        validate_portable_packages(&[package()]).unwrap();

        let mut invalid_base64 = package();
        invalid_base64.files[0].contents_base64 = "bWFuaWZlc3Q".into();
        assert!(validate_portable_packages(&[invalid_base64]).is_err());

        let mut duplicate = package();
        duplicate.files.push(duplicate.files[0].clone());
        assert!(validate_portable_packages(&[duplicate]).is_err());

        let unknown = serde_json::json!({
            "path": "manifest.toml",
            "contents_base64": STANDARD.encode(b"manifest"),
            "sha256": "not-supported"
        });
        assert!(serde_json::from_value::<PortableProtocolPackageFile>(unknown).is_err());
    }

    #[test]
    fn unsafe_paths_and_duplicate_identities_are_rejected() {
        for path in ["/manifest.toml", "../manifest.toml", "a\\b", "C:manifest"] {
            let mut value = package();
            value.files[0].path = path.into();
            assert!(validate_portable_packages(&[value]).is_err(), "{path}");
        }
        let value = package();
        assert!(validate_portable_packages(&[value.clone(), value]).is_err());
    }

    #[test]
    fn package_list_requires_id_then_semver_order() {
        let mut first = package();
        first.package.version = ProtocolPackageVersion::new("2.0.0").unwrap();
        let mut second = package();
        second.package.version = ProtocolPackageVersion::new("10.0.0").unwrap();
        validate_portable_packages(&[first.clone(), second.clone()]).unwrap();
        assert!(validate_portable_packages(&[second, first]).is_err());

        let mut later_id = package();
        later_id.package.id = ProtocolPackageId::new("z-package").unwrap();
        let mut earlier_id = package();
        earlier_id.package.id = ProtocolPackageId::new("a-package").unwrap();
        assert!(validate_portable_packages(&[later_id, earlier_id]).is_err());
    }

    #[test]
    fn package_and_file_count_limits_accept_the_boundary_and_reject_one_more() {
        let packages = (1..=MAX_PORTABLE_PROTOCOL_PACKAGES + 1)
            .map(|major| {
                let mut value = package();
                value.package.version =
                    ProtocolPackageVersion::new(format!("{major}.0.0")).unwrap();
                value
            })
            .collect::<Vec<_>>();
        validate_portable_packages(&packages[..MAX_PORTABLE_PROTOCOL_PACKAGES]).unwrap();
        assert!(validate_portable_packages(&packages).is_err());

        let mut value = package();
        value.files = (0..=MAX_PORTABLE_PACKAGE_FILES)
            .map(|index| PortableProtocolPackageFile {
                path: format!("{index:04}.rhai"),
                contents_base64: String::new(),
            })
            .collect();
        validate_portable_packages(std::slice::from_ref(&PortableProtocolPackage {
            files: value.files[..MAX_PORTABLE_PACKAGE_FILES].to_vec(),
            ..value.clone()
        }))
        .unwrap();
        assert!(validate_portable_packages(&[value]).is_err());
    }

    #[test]
    fn file_and_package_byte_limits_are_exact() {
        let mut at_file_limit = package();
        at_file_limit.files[0].contents_base64 =
            STANDARD.encode(vec![0_u8; MAX_PORTABLE_PACKAGE_FILE_BYTES]);
        validate_portable_packages(std::slice::from_ref(&at_file_limit)).unwrap();

        let mut over_file_limit = package();
        over_file_limit.files[0].contents_base64 =
            STANDARD.encode(vec![0_u8; MAX_PORTABLE_PACKAGE_FILE_BYTES + 1]);
        assert!(validate_portable_packages(&[over_file_limit]).is_err());

        let mut at_total_limit = package();
        at_total_limit.files = (0..4)
            .map(|index| PortableProtocolPackageFile {
                path: format!("{index}.bin"),
                contents_base64: STANDARD.encode(vec![0_u8; MAX_PORTABLE_PACKAGE_FILE_BYTES]),
            })
            .collect();
        validate_portable_packages(std::slice::from_ref(&at_total_limit)).unwrap();

        at_total_limit.files.push(PortableProtocolPackageFile {
            path: "4.bin".into(),
            contents_base64: STANDARD.encode([0_u8]),
        });
        assert!(validate_portable_packages(&[at_total_limit]).is_err());
    }

    #[test]
    fn path_length_and_depth_limits_are_exact() {
        let mut value = package();
        value.files[0].path = "a".repeat(MAX_PORTABLE_PACKAGE_PATH_BYTES);
        validate_portable_packages(std::slice::from_ref(&value)).unwrap();
        value.files[0].path.push('a');
        assert!(validate_portable_packages(std::slice::from_ref(&value)).is_err());

        value.files[0].path = std::iter::repeat_n("a", MAX_PORTABLE_PACKAGE_PATH_DEPTH)
            .collect::<Vec<_>>()
            .join("/");
        validate_portable_packages(std::slice::from_ref(&value)).unwrap();
        value.files[0].path.push_str("/a");
        assert!(validate_portable_packages(&[value]).is_err());
    }

    #[test]
    fn application_requires_referenced_packages_and_allows_unreferenced_installed_packages() {
        let value = package();
        let workspace = scripted_workspace(value.package.clone());
        assert!(
            validate_configuration_package_references(
                std::slice::from_ref(&workspace),
                &[] as &[PortableProtocolPackage],
                true,
            )
            .is_err()
        );

        let unused = PortableProtocolPackage {
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new("unused").unwrap(),
                version: ProtocolPackageVersion::new("2.0.0").unwrap(),
            },
            files: value.files.clone(),
        };
        validate_configuration_package_references(&[workspace], &[value, unused], true).unwrap();
    }
}
