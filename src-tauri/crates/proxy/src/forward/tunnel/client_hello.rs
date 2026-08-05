//! TLS `ClientHello` 的有界预读、跨 record 重组与 ALPN 检查。

use std::time::Duration;

use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

use super::timeout_or_cancel;
use crate::forward::config_error;
use crate::{ErrorCode, ProxyError, Result};

const MAX_CLIENT_HELLO_BYTES: usize = 64 * 1024;

pub(in crate::forward) async fn read_client_hello_prefix<T: AsyncRead + Unpin>(
    io: &mut T,
    read_timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<Bytes> {
    let mut records = Vec::new();
    let mut handshake = Vec::new();
    loop {
        let mut header = [0u8; 5];
        timeout_or_cancel(
            read_timeout,
            cancellation,
            io.read_exact(&mut header),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("read TLS ClientHello header", &error))?;
        let record_len = usize::from(u16::from_be_bytes([header[3], header[4]]));
        if header[0] != 22
            || record_len == 0
            || records
                .len()
                .saturating_add(header.len())
                .saturating_add(record_len)
                > MAX_CLIENT_HELLO_BYTES
        {
            return Err(config_error("TLS ClientHello record is invalid"));
        }
        let mut payload = vec![0; record_len];
        timeout_or_cancel(
            read_timeout,
            cancellation,
            io.read_exact(&mut payload),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("read TLS ClientHello body", &error))?;
        records.extend_from_slice(&header);
        records.extend_from_slice(&payload);
        handshake.extend_from_slice(&payload);

        if handshake.len() >= 4 {
            if handshake[0] != 1 {
                return Err(config_error("first TLS handshake is not ClientHello"));
            }
            let handshake_len = (usize::from(handshake[1]) << 16)
                | (usize::from(handshake[2]) << 8)
                | usize::from(handshake[3]);
            let total = 4usize.saturating_add(handshake_len);
            if total > MAX_CLIENT_HELLO_BYTES {
                return Err(config_error("TLS ClientHello exceeds the configured limit"));
            }
            if handshake.len() >= total {
                return Ok(Bytes::from(records));
            }
        }
    }
}

pub(in crate::forward) fn client_hello_requires_tunnel(record: &[u8]) -> bool {
    client_hello_alpn_protocols(record).is_some_and(|protocols| {
        protocols
            .iter()
            .any(|protocol| protocol.as_slice() == b"h2" || protocol.starts_with(b"h3"))
    })
}

fn client_hello_alpn_protocols(record: &[u8]) -> Option<Vec<Vec<u8>>> {
    let payload = collect_client_hello_handshake(record)?;
    if payload.len() < 4 || payload[0] != 1 {
        return None;
    }
    let handshake_len =
        (usize::from(payload[1]) << 16) | (usize::from(payload[2]) << 8) | usize::from(payload[3]);
    let hello = payload.get(4..4usize.checked_add(handshake_len)?)?;
    let mut offset = 2usize.checked_add(32)?;
    let session_len = usize::from(*hello.get(offset)?);
    offset = offset.checked_add(1 + session_len)?;
    let cipher_len = usize::from(u16::from_be_bytes([
        *hello.get(offset)?,
        *hello.get(offset + 1)?,
    ]));
    offset = offset.checked_add(2 + cipher_len)?;
    let compression_len = usize::from(*hello.get(offset)?);
    offset = offset.checked_add(1 + compression_len)?;
    let extensions_len = usize::from(u16::from_be_bytes([
        *hello.get(offset)?,
        *hello.get(offset + 1)?,
    ]));
    offset = offset.checked_add(2)?;
    let extensions = hello.get(offset..offset.checked_add(extensions_len)?)?;
    let mut extension_offset = 0usize;
    while extension_offset + 4 <= extensions.len() {
        let kind = u16::from_be_bytes([
            extensions[extension_offset],
            extensions[extension_offset + 1],
        ]);
        let length = usize::from(u16::from_be_bytes([
            extensions[extension_offset + 2],
            extensions[extension_offset + 3],
        ]));
        extension_offset += 4;
        let data = extensions.get(extension_offset..extension_offset.checked_add(length)?)?;
        extension_offset += length;
        if kind == 16 {
            let list_len = usize::from(u16::from_be_bytes([*data.first()?, *data.get(1)?]));
            let list = data.get(2..2usize.checked_add(list_len)?)?;
            let mut protocols = Vec::new();
            let mut protocol_offset = 0usize;
            while protocol_offset < list.len() {
                let length = usize::from(*list.get(protocol_offset)?);
                protocol_offset += 1;
                let protocol = list.get(protocol_offset..protocol_offset.checked_add(length)?)?;
                protocols.push(protocol.to_vec());
                protocol_offset += length;
            }
            return Some(protocols);
        }
    }
    None
}

/// `ClientHello` 可能跨多个 TLS record，先重组完整握手再做协议判定。
fn collect_client_hello_handshake(records: &[u8]) -> Option<Vec<u8>> {
    let mut offset = 0usize;
    let mut handshake = Vec::new();
    while offset < records.len() {
        let header = records.get(offset..offset.checked_add(5)?)?;
        if header[0] != 22 {
            return None;
        }
        let length = usize::from(u16::from_be_bytes([header[3], header[4]]));
        offset = offset.checked_add(5)?;
        let payload = records.get(offset..offset.checked_add(length)?)?;
        handshake.extend_from_slice(payload);
        offset = offset.checked_add(length)?;
        if handshake.len() >= 4 {
            let length = (usize::from(handshake[1]) << 16)
                | (usize::from(handshake[2]) << 8)
                | usize::from(handshake[3]);
            let total = 4usize.checked_add(length)?;
            if handshake.len() >= total {
                handshake.truncate(total);
                return Some(handshake);
            }
        }
    }
    None
}
