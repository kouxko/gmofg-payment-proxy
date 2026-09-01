//! v4 可移植文档中的协议包文件载荷。
//!
//! 这里只验证 JSON wire 自身可以安全、确定地交给恢复层：身份唯一、路径规范、Base64
//! 规范且资源有界。Component、Manifest、Schema 和完整的跨平台路径冲突检查仍由
//! infrastructure 的协议包运行时完成，Application 不复制第二套解析器。

use std::{cmp::Ordering, collections::HashSet};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use intercept_proxy_domain::{ProtocolPackageRef, ProxyWorkspace};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{AppError, AppResult};

pub const MAX_PORTABLE_PROTOCOL_PACKAGES: usize = 256;
pub const PORTABLE_COMPONENT_PATH: &str = "component.wasm";

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
    if files.len() != 1 || files[0].path != PORTABLE_COMPONENT_PATH {
        return Err(invalid("协议包必须且只能包含 component.wasm。"));
    }
    let decoded = STANDARD
        .decode(&files[0].contents_base64)
        .map_err(|_| invalid("协议包文件内容不是有效的标准 Base64。"))?;
    if STANDARD.encode(&decoded) != files[0].contents_base64 {
        return Err(invalid("协议包文件内容必须使用规范的标准 Base64 编码。"));
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
            "完整配置中的全部 Scripted Listener 和 协议报文规则都必须引用内嵌协议包。",
        ))
    }
}

fn referenced_packages(workspaces: &[ProxyWorkspace]) -> HashSet<ProtocolPackageRef> {
    workspaces
        .iter()
        .flat_map(|workspace| workspace.listeners.iter())
        .filter_map(crate::listener_protocol_package)
        .cloned()
        .collect()
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::new("PORTABLE_PROTOCOL_PACKAGE_INVALID", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use intercept_proxy_domain::{
        ListenerDataPlane, ProtocolPackageId, ProtocolPackageVersion, ProxyListener,
        ScriptedSocketProcessing, SocketEndpoint, SocketPayloadProcessing, SocketRelaySecurity,
        SocketRelaySettings,
    };

    fn package() -> PortableProtocolPackage {
        PortableProtocolPackage {
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new("example").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            files: vec![PortableProtocolPackageFile {
                path: PORTABLE_COMPONENT_PATH.into(),
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
                SocketPayloadProcessing::Scripted(ScriptedSocketProcessing { package }),
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
            "path": "manifest.json",
            "contents_base64": STANDARD.encode(b"manifest"),
            "sha256": "not-supported"
        });
        assert!(serde_json::from_value::<PortableProtocolPackageFile>(unknown).is_err());
    }

    #[test]
    fn unsafe_paths_and_duplicate_identities_are_rejected() {
        for path in ["/manifest.json", "../manifest.json", "a\\b", "C:manifest"] {
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
    fn package_count_limit_accepts_the_boundary_and_rejects_one_more() {
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
    }

    #[test]
    fn package_requires_one_component_and_has_no_component_byte_limit() {
        let mut large = package();
        large.files[0].contents_base64 = STANDARD.encode(vec![0_u8; 9 * 1024 * 1024]);
        validate_portable_packages(std::slice::from_ref(&large)).unwrap();

        let mut wrong_path = package();
        wrong_path.files[0].path = "protocol.js".into();
        assert!(validate_portable_packages(&[wrong_path]).is_err());

        let mut multiple = package();
        multiple.files.push(PortableProtocolPackageFile {
            path: "manifest.json".into(),
            contents_base64: STANDARD.encode(b"manifest"),
        });
        assert!(validate_portable_packages(&[multiple]).is_err());
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
