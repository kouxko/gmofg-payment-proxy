use std::collections::BTreeSet;

use intercept_proxy_domain::{
    DomainError, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
use serde::Deserialize;

use crate::{
    PackageFilePath, ProtocolFunctionName, ProtocolPackageFile, ProtocolPackageParseError,
    ProtocolPackageParseErrorCode, toml_parser::parse_toml,
};

/// 当前协议脚本宿主实现的 Manifest API 主版本。
pub const SUPPORTED_PROTOCOL_HOST_API: u32 = 1;
/// `manifest.toml` 的独立解析上限；ZIP 总量门禁属于 T07。
pub const MAX_MANIFEST_TOML_BYTES: usize = 64 * 1024;

/// Manifest 中可展示的协议包身份。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolPackageMetadata {
    package: ProtocolPackageRef,
    name: String,
}

impl ProtocolPackageMetadata {
    /// 返回 Listener 绑定和注册表索引使用的精确包身份。
    #[must_use]
    pub const fn package(&self) -> &ProtocolPackageRef {
        &self.package
    }

    /// 返回协议包列表和详情 Dialog 使用的作者名称。
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// 可选的 Document 自定义展示入口。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayDeclaration {
    script: PackageFilePath,
    function: ProtocolFunctionName,
}

impl DisplayDeclaration {
    /// 返回实现 Display 的包内脚本。
    #[must_use]
    pub const fn script(&self) -> &PackageFilePath {
        &self.script
    }

    /// 返回由宿主调用的 Display 函数名。
    #[must_use]
    pub const fn function(&self) -> &ProtocolFunctionName {
        &self.function
    }
}

/// Manifest 的 Document Schema 与可选展示声明。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentDeclaration {
    schema: PackageFilePath,
    display: Option<DisplayDeclaration>,
}

impl DocumentDeclaration {
    /// 返回提前声明规则变量的 Schema 文件。
    #[must_use]
    pub const fn schema(&self) -> &PackageFilePath {
        &self.schema
    }

    /// 返回可选 Display 能力；未声明时 UI 必须回退 Hex。
    #[must_use]
    pub const fn display(&self) -> Option<&DisplayDeclaration> {
        self.display.as_ref()
    }
}

/// 单方向必需的完整 Frame 与 Decode 入口。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiveHookDeclaration {
    script: PackageFilePath,
    frame: ProtocolFunctionName,
    decode: ProtocolFunctionName,
}

impl ReceiveHookDeclaration {
    /// 返回同时实现该方向 Frame/Decode 的脚本。
    #[must_use]
    pub const fn script(&self) -> &PackageFilePath {
        &self.script
    }

    /// 返回从方向私有 FIFO 取得完整 Frame 的入口名。
    #[must_use]
    pub const fn frame(&self) -> &ProtocolFunctionName {
        &self.frame
    }

    /// 返回把完整 Frame 解码为 Document 的入口名。
    #[must_use]
    pub const fn decode(&self) -> &ProtocolFunctionName {
        &self.decode
    }
}

/// 单方向可选的 Encode 入口。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SendHookDeclaration {
    script: PackageFilePath,
    encode: ProtocolFunctionName,
}

impl SendHookDeclaration {
    /// 返回实现该方向 Encode 的脚本。
    #[must_use]
    pub const fn script(&self) -> &PackageFilePath {
        &self.script
    }

    /// 返回编码完整输出 Frame 的入口名。
    #[must_use]
    pub const fn encode(&self) -> &ProtocolFunctionName {
        &self.encode
    }
}

/// Upstream 或 Downstream 的入口集合。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectionHooks {
    receive: ReceiveHookDeclaration,
    send: Option<SendHookDeclaration>,
}

impl DirectionHooks {
    /// 返回始终需要的 Frame/Decode 声明。
    #[must_use]
    pub const fn receive(&self) -> &ReceiveHookDeclaration {
        &self.receive
    }

    /// 返回可选 Encode 声明；`None` 表示该方向不支持 Encode，UI 开关必须固定关闭。
    #[must_use]
    pub const fn send(&self) -> Option<&SendHookDeclaration> {
        self.send.as_ref()
    }
}

/// App 到 Server 与 Server 到 App 两个互相隔离的方向声明。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolHooks {
    upstream: DirectionHooks,
    downstream: DirectionHooks,
}

impl ProtocolHooks {
    /// 返回 App -> Proxy -> Server 的入口。
    #[must_use]
    pub const fn upstream(&self) -> &DirectionHooks {
        &self.upstream
    }

    /// 返回 Server -> Proxy -> App 的入口。
    #[must_use]
    pub const fn downstream(&self) -> &DirectionHooks {
        &self.downstream
    }
}

/// 通过严格 TOML 与领域校验后、可交给 ZIP/Rhai 编译阶段的 Manifest。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolManifest {
    api: u32,
    package: ProtocolPackageMetadata,
    document: DocumentDeclaration,
    hooks: ProtocolHooks,
}

impl ProtocolManifest {
    /// 返回已经确认受宿主支持的 API 版本。
    #[must_use]
    pub const fn api(&self) -> u32 {
        self.api
    }

    /// 返回协议包身份和展示名称。
    #[must_use]
    pub const fn package(&self) -> &ProtocolPackageMetadata {
        &self.package
    }

    /// 返回 Schema 与可选 Display 声明。
    #[must_use]
    pub const fn document(&self) -> &DocumentDeclaration {
        &self.document
    }

    /// 返回两个方向互相独立的入口声明。
    #[must_use]
    pub const fn hooks(&self) -> &ProtocolHooks {
        &self.hooks
    }

    /// 按 Manifest 语义顺序返回全部引用文件；同一脚本被多个入口复用时会去重。
    #[must_use]
    pub fn referenced_files(&self) -> BTreeSet<&PackageFilePath> {
        self.file_references()
            .into_iter()
            .map(|(_, path)| path)
            .collect()
    }

    /// 检查 Manifest 的每个 Schema/脚本引用是否存在于已验证文件集合。
    ///
    /// 本方法不读取 ZIP，也不接受原始路径字符串。T07 必须先把 ZIP 条目转换成
    /// [`PackageFilePath`]，再调用此方法完成“Manifest 未声明文件”矩阵。
    pub fn validate_referenced_files(
        &self,
        available: &BTreeSet<PackageFilePath>,
    ) -> Result<(), ProtocolPackageParseError> {
        if let Some((field, _)) = self
            .file_references()
            .into_iter()
            .find(|(_, path)| !available.contains(*path))
        {
            return Err(ProtocolPackageParseError::new(
                ProtocolPackageParseErrorCode::ReferencedFileMissing,
                ProtocolPackageFile::Manifest,
                field,
            ));
        }
        Ok(())
    }

    fn file_references(&self) -> Vec<(&'static str, &PackageFilePath)> {
        let mut references = vec![("document.schema", &self.document.schema)];
        if let Some(display) = &self.document.display {
            references.push(("document.display.script", &display.script));
        }
        references.push((
            "hooks.upstream.receive.script",
            &self.hooks.upstream.receive.script,
        ));
        if let Some(send) = &self.hooks.upstream.send {
            references.push(("hooks.upstream.send.script", &send.script));
        }
        references.push((
            "hooks.downstream.receive.script",
            &self.hooks.downstream.receive.script,
        ));
        if let Some(send) = &self.hooks.downstream.send {
            references.push(("hooks.downstream.send.script", &send.script));
        }
        references
    }
}

/// 严格解析 `manifest.toml` 并形成受校验声明模型。
///
/// 未知键、错误类型、重复键和缺少必需方向/入口统一返回 `TOML_INVALID`；API、身份、路径、名称
/// 等语义错误使用更精确的稳定代码。任何错误都不会包含 TOML 原文。
pub fn parse_protocol_manifest(input: &str) -> Result<ProtocolManifest, ProtocolPackageParseError> {
    let wire: ManifestWire = parse_toml(
        input,
        ProtocolPackageFile::Manifest,
        MAX_MANIFEST_TOML_BYTES,
    )?;
    ProtocolManifest::try_from(wire)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestWire {
    api: u32,
    package: PackageWire,
    document: DocumentWire,
    hooks: HooksWire,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageWire {
    id: String,
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentWire {
    schema: String,
    display: Option<DisplayWire>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayWire {
    script: String,
    function: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HooksWire {
    upstream: DirectionWire,
    downstream: DirectionWire,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectionWire {
    receive: ReceiveWire,
    send: Option<SendWire>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiveWire {
    script: String,
    frame: String,
    decode: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SendWire {
    script: String,
    encode: String,
}

impl TryFrom<ManifestWire> for ProtocolManifest {
    type Error = ProtocolPackageParseError;

    fn try_from(wire: ManifestWire) -> Result<Self, Self::Error> {
        if wire.api != SUPPORTED_PROTOCOL_HOST_API {
            return Err(ProtocolPackageParseError::new(
                ProtocolPackageParseErrorCode::UnsupportedHostApi,
                ProtocolPackageFile::Manifest,
                "api",
            ));
        }
        let package = ProtocolPackageMetadata {
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new(wire.package.id)
                    .map_err(|error| domain_error(&error, "package.id"))?,
                version: ProtocolPackageVersion::new(wire.package.version)
                    .map_err(|error| domain_error(&error, "package.version"))?,
            },
            name: validate_display_name(wire.package.name)?,
        };
        let document = DocumentDeclaration {
            schema: PackageFilePath::new_for_field(wire.document.schema, "document.schema")?,
            display: wire
                .document
                .display
                .map(DisplayDeclaration::try_from)
                .transpose()?,
        };
        Ok(Self {
            api: wire.api,
            package,
            document,
            hooks: ProtocolHooks {
                upstream: direction_from_wire(wire.hooks.upstream, "hooks.upstream")?,
                downstream: direction_from_wire(wire.hooks.downstream, "hooks.downstream")?,
            },
        })
    }
}

impl TryFrom<DisplayWire> for DisplayDeclaration {
    type Error = ProtocolPackageParseError;

    fn try_from(wire: DisplayWire) -> Result<Self, Self::Error> {
        Ok(Self {
            script: PackageFilePath::new_for_field(wire.script, "document.display.script")?,
            function: ProtocolFunctionName::new_for_field(
                wire.function,
                "document.display.function",
            )?,
        })
    }
}

fn direction_from_wire(
    wire: DirectionWire,
    prefix: &str,
) -> Result<DirectionHooks, ProtocolPackageParseError> {
    let receive_prefix = format!("{prefix}.receive");
    let receive = ReceiveHookDeclaration {
        script: PackageFilePath::new_for_field(
            wire.receive.script,
            &format!("{receive_prefix}.script"),
        )?,
        frame: ProtocolFunctionName::new_for_field(
            wire.receive.frame,
            &format!("{receive_prefix}.frame"),
        )?,
        decode: ProtocolFunctionName::new_for_field(
            wire.receive.decode,
            &format!("{receive_prefix}.decode"),
        )?,
    };
    let send = wire
        .send
        .map(|send| {
            let send_prefix = format!("{prefix}.send");
            Ok(SendHookDeclaration {
                script: PackageFilePath::new_for_field(
                    send.script,
                    &format!("{send_prefix}.script"),
                )?,
                encode: ProtocolFunctionName::new_for_field(
                    send.encode,
                    &format!("{send_prefix}.encode"),
                )?,
            })
        })
        .transpose()?;
    Ok(DirectionHooks { receive, send })
}

fn validate_display_name(name: String) -> Result<String, ProtocolPackageParseError> {
    if name.trim().is_empty() || name.chars().count() > 128 || name.chars().any(char::is_control) {
        return Err(ProtocolPackageParseError::new(
            ProtocolPackageParseErrorCode::ManifestInvalid,
            ProtocolPackageFile::Manifest,
            "package.name",
        ));
    }
    Ok(name)
}

fn domain_error(error: &DomainError, fallback: &str) -> ProtocolPackageParseError {
    let field = error
        .field_errors
        .keys()
        .next()
        .map_or(fallback, String::as_str);
    ProtocolPackageParseError::new(
        ProtocolPackageParseErrorCode::ManifestInvalid,
        ProtocolPackageFile::Manifest,
        field,
    )
}
