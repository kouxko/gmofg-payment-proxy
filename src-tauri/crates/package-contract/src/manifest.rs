use intercept_proxy_domain::{
    DocumentSchemaNode, DomainError, ErrorCode, MAX_DOCUMENT_SCHEMA_TITLE_CHARS,
    MAX_PROTOCOL_PACKAGE_ID_LEN, MAX_PROTOCOL_PACKAGE_VERSION_LEN, PROTOCOL_PACKAGE_ID_PATTERN,
    ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
use serde::{Deserialize, Serialize};
use specta::Type;

/// The only supported package API major version.
pub const PACKAGE_API_V1: u32 = 1;
/// ECMA-compatible expression matching the full `SemVer` 2.0.0 syntax accepted by Domain.
pub const PROTOCOL_PACKAGE_SEMVER_PATTERN: &str = r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$";

/// Rust-owned values required by unknown-boundary validators in generated clients.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageContractValidation {
    /// Full package ID validation expression.
    pub package_id_pattern: &'static str,
    /// Maximum package ID byte length.
    pub package_id_max_bytes: usize,
    /// Full `SemVer` validation expression.
    pub package_version_pattern: &'static str,
    /// Maximum package version byte length.
    pub package_version_max_bytes: usize,
    /// Maximum numeric value accepted for each core `SemVer` component.
    pub package_version_core_numeric_max: String,
    /// Maximum Schema title Unicode character count.
    pub schema_title_max_chars: usize,
    /// Complete Domain-owned stable error-code set.
    pub stable_error_codes: Vec<&'static str>,
}

/// Returns validation metadata generated alongside TypeScript contract bindings.
#[must_use]
pub fn package_contract_validation() -> PackageContractValidation {
    PackageContractValidation {
        package_id_pattern: PROTOCOL_PACKAGE_ID_PATTERN,
        package_id_max_bytes: MAX_PROTOCOL_PACKAGE_ID_LEN,
        package_version_pattern: PROTOCOL_PACKAGE_SEMVER_PATTERN,
        package_version_max_bytes: MAX_PROTOCOL_PACKAGE_VERSION_LEN,
        package_version_core_numeric_max: u64::MAX.to_string(),
        schema_title_max_chars: MAX_DOCUMENT_SCHEMA_TITLE_CHARS,
        stable_error_codes: ErrorCode::ALL.iter().map(|code| code.as_str()).collect(),
    }
}

fn invalid_manifest(field: &str, message: impl Into<String>) -> DomainError {
    DomainError::new(
        ErrorCode::ProtocolPackageInvalid,
        "package Manifest is invalid",
    )
    .with_field_error(field, message)
}

/// Protocol kind owned by one package version.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PackageKind {
    /// HTTP body package.
    Http,
    /// Socket frame package.
    Socket,
}

/// Exact package identity and human-readable metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PackageMetadata {
    id: ProtocolPackageId,
    version: ProtocolPackageVersion,
    name: String,
    description: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageMetadataInput {
    id: ProtocolPackageId,
    version: ProtocolPackageVersion,
    name: String,
    description: String,
}

impl PackageMetadata {
    /// Creates package metadata from its existing domain identity.
    pub fn new(
        identity: ProtocolPackageRef,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<Self, DomainError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(invalid_manifest(
                "package.name",
                "package name must contain a visible character",
            ));
        }
        Ok(Self {
            id: identity.id,
            version: identity.version,
            name,
            description: description.into(),
        })
    }

    /// Returns the exact package ID and version.
    #[must_use]
    pub fn identity(&self) -> ProtocolPackageRef {
        ProtocolPackageRef {
            id: self.id.clone(),
            version: self.version.clone(),
        }
    }

    /// Returns the visible package name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the author-supplied description; an empty description is valid.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }
}

impl<'de> Deserialize<'de> for PackageMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = PackageMetadataInput::deserialize(deserializer)?;
        Self::new(
            ProtocolPackageRef {
                id: value.id,
                version: value.version,
            },
            value.name,
            value.description,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Optional Schema metadata for one HTTP or Socket direction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PackageDocumentDirection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema: Option<DocumentSchemaNode>,
}

impl PackageDocumentDirection {
    /// Creates an empty HTTP direction.
    #[must_use]
    pub const fn empty() -> Self {
        Self { schema: None }
    }

    /// Creates a direction with Schema metadata after validating the Schema definition.
    pub fn with_schema(schema: DocumentSchemaNode) -> Result<Self, DomainError> {
        schema.validate_definition()?;
        Ok(Self {
            schema: Some(schema),
        })
    }

    /// Returns the optional read-only Schema metadata.
    #[must_use]
    pub const fn schema(&self) -> Option<&DocumentSchemaNode> {
        self.schema.as_ref()
    }
}

/// Independent upstream and downstream Document metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PackageDocuments {
    upstream: PackageDocumentDirection,
    downstream: PackageDocumentDirection,
}

impl PackageDocuments {
    /// Creates direction metadata.
    #[must_use]
    pub const fn new(
        upstream: PackageDocumentDirection,
        downstream: PackageDocumentDirection,
    ) -> Self {
        Self {
            upstream,
            downstream,
        }
    }

    /// Returns upstream metadata.
    #[must_use]
    pub const fn upstream(&self) -> &PackageDocumentDirection {
        &self.upstream
    }

    /// Returns downstream metadata.
    #[must_use]
    pub const fn downstream(&self) -> &PackageDocumentDirection {
        &self.downstream
    }
}

/// Strict final API 1 package Manifest.
#[derive(Clone, Debug, Eq, PartialEq, Type)]
pub struct PackageManifest {
    api: u32,
    kind: PackageKind,
    package: PackageMetadata,
    document: PackageDocuments,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
struct PackageManifestWire {
    api: u32,
    kind: PackageKind,
    package: PackageMetadata,
    document: PackageDocuments,
}

impl PackageManifest {
    /// Creates a strict API 1 Manifest and validates only Manifest-local invariants.
    pub fn new(
        api: u32,
        kind: PackageKind,
        package: PackageMetadata,
        document: PackageDocuments,
    ) -> Result<Self, DomainError> {
        if api != PACKAGE_API_V1 {
            return Err(invalid_manifest("api", "only package API 1 is supported"));
        }
        for (field, direction) in [
            ("document.upstream.schema", document.upstream()),
            ("document.downstream.schema", document.downstream()),
        ] {
            if let Some(schema) = direction.schema() {
                schema
                    .validate_definition()
                    .map_err(|error| invalid_manifest(field, error.to_string()))?;
            }
        }
        if kind == PackageKind::Socket
            && (document.upstream().schema().is_none() || document.downstream().schema().is_none())
        {
            return Err(invalid_manifest(
                "document",
                "Socket packages require upstream and downstream schema",
            ));
        }
        Ok(Self {
            api,
            kind,
            package,
            document,
        })
    }

    /// Returns the fixed API major version.
    #[must_use]
    pub const fn api(&self) -> u32 {
        self.api
    }

    /// Returns the package protocol kind.
    #[must_use]
    pub const fn kind(&self) -> PackageKind {
        self.kind
    }

    /// Returns package identity and visible metadata.
    #[must_use]
    pub const fn package(&self) -> &PackageMetadata {
        &self.package
    }

    /// Returns direction Schema metadata.
    #[must_use]
    pub const fn document(&self) -> &PackageDocuments {
        &self.document
    }
}

impl TryFrom<PackageManifestWire> for PackageManifest {
    type Error = DomainError;

    fn try_from(value: PackageManifestWire) -> Result<Self, Self::Error> {
        Self::new(value.api, value.kind, value.package, value.document)
    }
}

impl From<PackageManifest> for PackageManifestWire {
    fn from(value: PackageManifest) -> Self {
        Self {
            api: value.api,
            kind: value.kind,
            package: value.package,
            document: value.document,
        }
    }
}

impl<'de> Deserialize<'de> for PackageManifest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        PackageManifestWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl Serialize for PackageManifest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        PackageManifestWire::from(self.clone()).serialize(serializer)
    }
}
