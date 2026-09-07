//! Transport-neutral protocol-package Hook boundary.
//!
//! Proxy pipelines exchange domain-native strings, Documents, and raw Socket bytes here. The
//! remote WebSocket adapter alone owns JSON-RPC DTOs and Base64 conversion; in-process Wasm never
//! crosses a transport encoding boundary.

use std::fmt;

use async_trait::async_trait;
use intercept_proxy_domain::{Document, ProtocolDirection};
use intercept_proxy_package_contract::{
    CanonicalBase64, DecodeParams, DisplayParams, EncodeParams, FrameParams, FrameResult,
};

use super::{PackageTransportClient, PackageTransportError};

#[async_trait]
pub(crate) trait ProtocolPackageRuntime: fmt::Debug + Send + Sync {
    async fn frame(
        &self,
        _direction: ProtocolDirection,
        _buffer: Vec<u8>,
    ) -> Result<FrameResult, PackageTransportError> {
        Err(unsupported_hook("frame"))
    }

    async fn decode_http(
        &self,
        _direction: ProtocolDirection,
        _input: String,
    ) -> Result<Document, PackageTransportError> {
        Err(unsupported_hook("decode_http"))
    }

    async fn encode_http(
        &self,
        _direction: ProtocolDirection,
        _original_input: String,
        _document: Document,
    ) -> Result<String, PackageTransportError> {
        Err(unsupported_hook("encode_http"))
    }

    async fn decode_socket(
        &self,
        _direction: ProtocolDirection,
        _input: Vec<u8>,
    ) -> Result<Document, PackageTransportError> {
        Err(unsupported_hook("decode_socket"))
    }

    async fn encode_socket(
        &self,
        _direction: ProtocolDirection,
        _original_input: Vec<u8>,
        _document: Document,
    ) -> Result<Vec<u8>, PackageTransportError> {
        Err(unsupported_hook("encode_socket"))
    }

    async fn display(
        &self,
        _direction: ProtocolDirection,
        _document: Document,
    ) -> Result<String, PackageTransportError> {
        Err(unsupported_hook("display"))
    }
}

fn unsupported_hook(hook: &str) -> PackageTransportError {
    PackageTransportError::Package {
        error: intercept_proxy_domain::DomainError::new(
            intercept_proxy_domain::ErrorCode::InternalError,
            format!("protocol package runtime does not implement {hook}"),
        ),
    }
}

fn package_error(error: intercept_proxy_domain::DomainError) -> PackageTransportError {
    PackageTransportError::Package { error }
}

/// Adapts the remote debugging transport to the same Hook boundary.
///
/// All JSON-RPC DTO construction and Base64 conversion stays in this implementation.
#[async_trait]
impl ProtocolPackageRuntime for PackageTransportClient {
    async fn frame(
        &self,
        direction: ProtocolDirection,
        buffer: Vec<u8>,
    ) -> Result<FrameResult, PackageTransportError> {
        let request = FrameParams {
            buffer: CanonicalBase64::from_bytes(&buffer),
        };
        match direction {
            ProtocolDirection::Upstream => self.upstream_frame(request).await,
            ProtocolDirection::Downstream => self.downstream_frame(request).await,
        }
    }

    async fn decode_http(
        &self,
        direction: ProtocolDirection,
        input: String,
    ) -> Result<Document, PackageTransportError> {
        let request = DecodeParams { input };
        match direction {
            ProtocolDirection::Upstream => self.upstream_decode(request).await,
            ProtocolDirection::Downstream => self.downstream_decode(request).await,
        }
    }

    async fn encode_http(
        &self,
        direction: ProtocolDirection,
        original_input: String,
        document: Document,
    ) -> Result<String, PackageTransportError> {
        let request = EncodeParams {
            original_input,
            document,
        };
        match direction {
            ProtocolDirection::Upstream => self.upstream_encode(request).await,
            ProtocolDirection::Downstream => self.downstream_encode(request).await,
        }
    }

    async fn decode_socket(
        &self,
        direction: ProtocolDirection,
        input: Vec<u8>,
    ) -> Result<Document, PackageTransportError> {
        self.decode_http(
            direction,
            CanonicalBase64::from_bytes(&input).as_str().to_owned(),
        )
        .await
    }

    async fn encode_socket(
        &self,
        direction: ProtocolDirection,
        original_input: Vec<u8>,
        document: Document,
    ) -> Result<Vec<u8>, PackageTransportError> {
        let encoded = self
            .encode_http(
                direction,
                CanonicalBase64::from_bytes(&original_input)
                    .as_str()
                    .to_owned(),
                document,
            )
            .await?;
        CanonicalBase64::try_from(encoded)
            .map(|value| value.bytes())
            .map_err(package_error)
    }

    async fn display(
        &self,
        direction: ProtocolDirection,
        document: Document,
    ) -> Result<String, PackageTransportError> {
        let request = DisplayParams { document };
        match direction {
            ProtocolDirection::Upstream => self.upstream_display(request).await,
            ProtocolDirection::Downstream => self.downstream_display(request).await,
        }
    }
}
