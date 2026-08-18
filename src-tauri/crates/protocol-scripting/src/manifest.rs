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
/// `manifest.toml` 的独立解析上限；ZIP 总量门禁属于归档层。
pub const MAX_MANIFEST_TOML_BYTES: usize = 64 * 1024;

const PROTOCOL_SCRIPT: &str = "protocol.rhai";
const DISPLAY_SCRIPT: &str = "display.rhai";

/// 协议包所属的数据平面。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolPackageKind {
    /// 文本 HTTP Body；不声明 Frame 入口。
    Http,
    /// 字节流 Socket 报文；两个方向都声明 Frame 入口。
    Socket,
}

/// Manifest 中可展示的协议包身份。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolPackageMetadata {
    package: ProtocolPackageRef,
    name: String,
}

impl ProtocolPackageMetadata {
    /// 返回绑定和注册表索引使用的精确包身份。
    #[must_use]
    pub const fn package(&self) -> &ProtocolPackageRef {
        &self.package
    }

    /// 返回协议包列表和详情使用的作者名称。
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// 单方向 Document 的展示入口。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayDeclaration {
    script: PackageFilePath,
    function: ProtocolFunctionName,
}

impl DisplayDeclaration {
    /// 返回固定的包内展示脚本。
    #[must_use]
    pub const fn script(&self) -> &PackageFilePath {
        &self.script
    }

    /// 返回由宿主调用的展示函数名。
    #[must_use]
    pub const fn function(&self) -> &ProtocolFunctionName {
        &self.function
    }
}

/// 单方向 Document Schema 与展示声明。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentDeclaration {
    schema: PackageFilePath,
    display: DisplayDeclaration,
}

impl DocumentDeclaration {
    /// 返回该方向规则和脚本共同使用的 Schema 文件。
    #[must_use]
    pub const fn schema(&self) -> &PackageFilePath {
        &self.schema
    }

    /// 返回该方向必需的展示入口。
    #[must_use]
    pub const fn display(&self) -> &DisplayDeclaration {
        &self.display
    }
}

/// 上行与下行相互独立的 Document 声明。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolDocuments {
    upstream: DocumentDeclaration,
    downstream: DocumentDeclaration,
}

impl ProtocolDocuments {
    /// 返回 App 到 Server 方向使用的 Schema 和展示入口。
    #[must_use]
    pub const fn upstream(&self) -> &DocumentDeclaration {
        &self.upstream
    }

    /// 返回 Server 到 App 方向使用的 Schema 和展示入口。
    #[must_use]
    pub const fn downstream(&self) -> &DocumentDeclaration {
        &self.downstream
    }
}

/// 单方向固定脚本中的处理入口。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectionHooks {
    script: PackageFilePath,
    frame: Option<ProtocolFunctionName>,
    decode: ProtocolFunctionName,
    encode: ProtocolFunctionName,
}

impl DirectionHooks {
    /// 返回固定的协议处理脚本。
    #[must_use]
    pub const fn script(&self) -> &PackageFilePath {
        &self.script
    }

    /// 返回 Socket Frame 入口；HTTP 包固定为 `None`。
    #[must_use]
    pub const fn frame(&self) -> Option<&ProtocolFunctionName> {
        self.frame.as_ref()
    }

    /// 返回把原始报文解码为 Document 的入口。
    #[must_use]
    pub const fn decode(&self) -> &ProtocolFunctionName {
        &self.decode
    }

    /// 返回把 Document 编码为发送内容的入口。
    #[must_use]
    pub const fn encode(&self) -> &ProtocolFunctionName {
        &self.encode
    }
}

/// App 到 Server 与 Server 到 App 两个互相隔离的方向声明。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolHooks {
    upstream: DirectionHooks,
    downstream: DirectionHooks,
}

impl ProtocolHooks {
    /// 返回 App 到 Server 方向入口。
    #[must_use]
    pub const fn upstream(&self) -> &DirectionHooks {
        &self.upstream
    }

    /// 返回 Server 到 App 方向入口。
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
    kind: ProtocolPackageKind,
    document: ProtocolDocuments,
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

    /// 返回严格推断的数据平面。
    #[must_use]
    pub const fn kind(&self) -> ProtocolPackageKind {
        self.kind
    }

    /// 返回上行与下行各自的 Schema 和展示声明。
    #[must_use]
    pub const fn document(&self) -> &ProtocolDocuments {
        &self.document
    }

    /// 返回两个方向互相独立的入口声明。
    #[must_use]
    pub const fn hooks(&self) -> &ProtocolHooks {
        &self.hooks
    }

    /// 按 Manifest 语义顺序返回全部引用文件；同一文件会去重。
    #[must_use]
    pub fn referenced_files(&self) -> BTreeSet<&PackageFilePath> {
        self.file_references()
            .into_iter()
            .map(|(_, path)| path)
            .collect()
    }

    /// 检查 Manifest 的每个 Schema/脚本引用是否存在于已验证文件集合。
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
        vec![
            ("document.upstream.schema", &self.document.upstream.schema),
            (
                "document.upstream.display",
                &self.document.upstream.display.script,
            ),
            (
                "document.downstream.schema",
                &self.document.downstream.schema,
            ),
            (
                "document.downstream.display",
                &self.document.downstream.display.script,
            ),
            ("hooks.upstream", &self.hooks.upstream.script),
            ("hooks.downstream", &self.hooks.downstream.script),
        ]
    }
}

/// 严格解析 `manifest.toml` 并形成受校验声明模型。
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
    document: DocumentsWire,
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
struct DocumentsWire {
    upstream: DocumentWire,
    downstream: DocumentWire,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentWire {
    schema: String,
    display: String,
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
    frame: Option<String>,
    decode: String,
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
        let upstream_has_frame = wire.hooks.upstream.frame.is_some();
        let downstream_has_frame = wire.hooks.downstream.frame.is_some();
        let kind = match (upstream_has_frame, downstream_has_frame) {
            (false, false) => ProtocolPackageKind::Http,
            (true, true) => ProtocolPackageKind::Socket,
            _ => return Err(manifest_error("hooks.frame")),
        };
        Ok(Self {
            api: wire.api,
            package,
            kind,
            document: ProtocolDocuments {
                upstream: document_from_wire(wire.document.upstream, "document.upstream")?,
                downstream: document_from_wire(wire.document.downstream, "document.downstream")?,
            },
            hooks: ProtocolHooks {
                upstream: direction_from_wire(wire.hooks.upstream, "hooks.upstream")?,
                downstream: direction_from_wire(wire.hooks.downstream, "hooks.downstream")?,
            },
        })
    }
}

fn document_from_wire(
    wire: DocumentWire,
    prefix: &str,
) -> Result<DocumentDeclaration, ProtocolPackageParseError> {
    Ok(DocumentDeclaration {
        schema: PackageFilePath::new_for_field(wire.schema, &format!("{prefix}.schema"))?,
        display: DisplayDeclaration {
            script: PackageFilePath::new_for_field(
                DISPLAY_SCRIPT.to_owned(),
                &format!("{prefix}.display"),
            )?,
            function: ProtocolFunctionName::new_for_field(
                wire.display,
                &format!("{prefix}.display"),
            )?,
        },
    })
}

fn direction_from_wire(
    wire: DirectionWire,
    prefix: &str,
) -> Result<DirectionHooks, ProtocolPackageParseError> {
    Ok(DirectionHooks {
        script: PackageFilePath::new_for_field(PROTOCOL_SCRIPT.to_owned(), prefix)?,
        frame: wire
            .frame
            .map(|frame| ProtocolFunctionName::new_for_field(frame, &format!("{prefix}.frame")))
            .transpose()?,
        decode: ProtocolFunctionName::new_for_field(wire.decode, &format!("{prefix}.decode"))?,
        encode: ProtocolFunctionName::new_for_field(wire.encode, &format!("{prefix}.encode"))?,
    })
}

fn validate_display_name(name: String) -> Result<String, ProtocolPackageParseError> {
    if name.trim().is_empty() || name.chars().count() > 128 || name.chars().any(char::is_control) {
        return Err(manifest_error("package.name"));
    }
    Ok(name)
}

fn manifest_error(field: &str) -> ProtocolPackageParseError {
    ProtocolPackageParseError::new(
        ProtocolPackageParseErrorCode::ManifestInvalid,
        ProtocolPackageFile::Manifest,
        field,
    )
}

fn domain_error(error: &DomainError, fallback: &str) -> ProtocolPackageParseError {
    let field = error
        .field_errors
        .keys()
        .next()
        .map_or(fallback, String::as_str);
    manifest_error(field)
}
