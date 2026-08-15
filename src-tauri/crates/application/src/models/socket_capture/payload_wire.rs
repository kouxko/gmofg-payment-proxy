//! `SocketCapturePayload` 的严格反序列化边界。
//!
//! Serde 的相邻标签枚举默认可能忽略标签同级的额外字段。正式抓包属于证据数据，
//! 因此通过只在本模块可见的 wire 联合显式拒绝未知字段。

use serde::Deserialize;

use super::{SocketCapturePayload, SocketLocalExchangeCapture, SocketRelayFrameCapture};

impl<'de> Deserialize<'de> for SocketCapturePayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(
            tag = "kind",
            content = "capture",
            rename_all = "snake_case",
            deny_unknown_fields
        )]
        enum Wire {
            RelayFrame(SocketRelayFrameCapture),
            LocalExchange(SocketLocalExchangeCapture),
        }

        Ok(match Wire::deserialize(deserializer)? {
            Wire::RelayFrame(value) => Self::RelayFrame(value),
            Wire::LocalExchange(value) => Self::LocalExchange(value),
        })
    }
}
