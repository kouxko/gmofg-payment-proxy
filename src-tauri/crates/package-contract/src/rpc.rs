use base64::{Engine as _, engine::general_purpose::STANDARD};
use intercept_proxy_domain::{Document, DomainError, ErrorCode};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{FrameResult, PackageManifest};

/// Fixed JSON-RPC version.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum JsonRpcVersion {
    /// JSON-RPC 2.0.
    #[serde(rename = "2.0")]
    V2,
}

/// Canonical padded standard Base64 bytes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(transparent)]
pub struct CanonicalBase64(String);

impl CanonicalBase64 {
    /// Encodes bytes using canonical padded standard Base64.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(STANDARD.encode(bytes))
    }

    /// Returns the canonical wire text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Decodes the canonical wire text.
    #[must_use]
    pub fn bytes(&self) -> Vec<u8> {
        STANDARD
            .decode(self.0.as_bytes())
            .expect("CanonicalBase64 is validated at construction")
    }
}

impl TryFrom<String> for CanonicalBase64 {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let decoded = STANDARD.decode(value.as_bytes()).map_err(|_| {
            DomainError::new(ErrorCode::BodyDecodeFailed, "invalid Base64")
                .with_field_error("buffer", "must be canonical padded Base64")
        })?;
        if STANDARD.encode(decoded) != value {
            return Err(
                DomainError::new(ErrorCode::BodyDecodeFailed, "non-canonical Base64")
                    .with_field_error("buffer", "must be canonical padded Base64"),
            );
        }
        Ok(Self(value))
    }
}

impl<'de> Deserialize<'de> for CanonicalBase64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Parameters for a frame hook.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct FrameParams {
    /// Current accumulated Socket buffer.
    pub buffer: CanonicalBase64,
}

/// Parameters for a decode hook.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct DecodeParams {
    /// HTTP Unicode text or Socket canonical Base64, interpreted by the package kind adapter.
    pub input: String,
}

/// Parameters for an encode hook.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EncodeParams {
    /// Original HTTP text or Socket Base64 input.
    pub original_input: String,
    /// Natural recursive JSON Document.
    pub document: Document,
}

/// Parameters for a display hook.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct DisplayParams {
    /// Natural recursive JSON Document.
    pub document: Document,
}

/// Every fixed package hook request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "method", deny_unknown_fields)]
pub enum PackageRpcRequest {
    /// Upstream framing.
    #[serde(rename = "hooks.upstream.frame")]
    UpstreamFrame {
        /// JSON-RPC 2.0 marker.
        jsonrpc: JsonRpcVersion,
        /// Established string request ID.
        id: String,
        /// Frame parameters.
        params: FrameParams,
    },
    /// Downstream framing.
    #[serde(rename = "hooks.downstream.frame")]
    DownstreamFrame {
        /// JSON-RPC 2.0 marker.
        jsonrpc: JsonRpcVersion,
        /// Established string request ID.
        id: String,
        /// Frame parameters.
        params: FrameParams,
    },
    /// Upstream decode.
    #[serde(rename = "hooks.upstream.decode")]
    UpstreamDecode {
        /// JSON-RPC 2.0 marker.
        jsonrpc: JsonRpcVersion,
        /// Established string request ID.
        id: String,
        /// Decode parameters.
        params: DecodeParams,
    },
    /// Downstream decode.
    #[serde(rename = "hooks.downstream.decode")]
    DownstreamDecode {
        /// JSON-RPC 2.0 marker.
        jsonrpc: JsonRpcVersion,
        /// Established string request ID.
        id: String,
        /// Decode parameters.
        params: DecodeParams,
    },
    /// Upstream encode.
    #[serde(rename = "hooks.upstream.encode")]
    UpstreamEncode {
        /// JSON-RPC 2.0 marker.
        jsonrpc: JsonRpcVersion,
        /// Established string request ID.
        id: String,
        /// Encode parameters.
        params: EncodeParams,
    },
    /// Downstream encode.
    #[serde(rename = "hooks.downstream.encode")]
    DownstreamEncode {
        /// JSON-RPC 2.0 marker.
        jsonrpc: JsonRpcVersion,
        /// Established string request ID.
        id: String,
        /// Encode parameters.
        params: EncodeParams,
    },
    /// Upstream Document display.
    #[serde(rename = "document.upstream.display")]
    UpstreamDisplay {
        /// JSON-RPC 2.0 marker.
        jsonrpc: JsonRpcVersion,
        /// Established string request ID.
        id: String,
        /// Display parameters.
        params: DisplayParams,
    },
    /// Downstream Document display.
    #[serde(rename = "document.downstream.display")]
    DownstreamDisplay {
        /// JSON-RPC 2.0 marker.
        jsonrpc: JsonRpcVersion,
        /// Established string request ID.
        id: String,
        /// Display parameters.
        params: DisplayParams,
    },
}

/// One-way package registration notification. Its strict shape cannot contain an `id`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PackageRegisterNotification {
    jsonrpc: JsonRpcVersion,
    method: PackageRegisterMethod,
    params: PackageManifest,
}

/// The only registration method.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum PackageRegisterMethod {
    /// `package.register`.
    #[serde(rename = "package.register")]
    Register,
}

impl PackageRegisterNotification {
    /// Creates an id-less registration notification.
    #[must_use]
    pub const fn new(params: PackageManifest) -> Self {
        Self {
            jsonrpc: JsonRpcVersion::V2,
            method: PackageRegisterMethod::Register,
            params,
        }
    }

    /// Returns the full Manifest params.
    #[must_use]
    pub const fn params(&self) -> &PackageManifest {
        &self.params
    }
}

/// Strict successful JSON-RPC response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PackageRpcSuccess<R> {
    /// JSON-RPC 2.0 marker.
    pub jsonrpc: JsonRpcVersion,
    /// String request ID copied from the request.
    pub id: String,
    /// Method-specific result.
    pub result: R,
}

/// Stable-code error data shared with Domain and UI.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PackageRpcErrorData {
    code: ErrorCode,
}

impl PackageRpcErrorData {
    /// Returns the stable machine code.
    #[must_use]
    pub const fn code(self) -> ErrorCode {
        self.code
    }
}

/// Strict JSON-RPC error object.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PackageRpcError {
    code: i64,
    message: String,
    data: PackageRpcErrorData,
}

impl PackageRpcError {
    /// Creates an error containing a stable Domain/UI code.
    #[must_use]
    pub fn new(code: i64, message: impl Into<String>, stable_code: ErrorCode) -> Self {
        Self {
            code,
            message: message.into(),
            data: PackageRpcErrorData { code: stable_code },
        }
    }

    /// Returns stable error data.
    #[must_use]
    pub const fn data(&self) -> PackageRpcErrorData {
        self.data
    }

    /// Returns the JSON-RPC numeric error code.
    #[must_use]
    pub const fn code(&self) -> i64 {
        self.code
    }

    /// Returns the package-provided error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Strict failed JSON-RPC response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PackageRpcFailure {
    /// JSON-RPC 2.0 marker.
    pub jsonrpc: JsonRpcVersion,
    /// String request ID copied from the request.
    pub id: String,
    /// Typed error object.
    pub error: PackageRpcError,
}

/// Result shape of a frame hook.
pub type FrameRpcSuccess = PackageRpcSuccess<FrameResult>;
/// Result shape of a decode hook.
pub type DecodeRpcSuccess = PackageRpcSuccess<Document>;
/// Result shape of an encode hook.
pub type EncodeRpcSuccess = PackageRpcSuccess<String>;
/// Result shape of a display hook.
pub type DisplayRpcSuccess = PackageRpcSuccess<String>;
