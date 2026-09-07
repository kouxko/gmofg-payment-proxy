//! 正向 HTTP 目标到通用 `UpstreamConnector` 的适配。

use std::{fmt, time::Duration};

use async_trait::async_trait;
use http::header::{HOST, HeaderValue};
use tokio_util::sync::CancellationToken;

use super::{ErrorCode, MessageLimits, ProxyError, connect_target, strip_hop_by_hop_headers};
use crate::Result;
use crate::fault::FaultAction;
use crate::http::{
    ForwardRequest, InformationalResponseSink, PipelinePorts, UpstreamConnector, UpstreamExchange,
    UpstreamWriteReceipt,
};
use crate::message::Message;
use crate::transport::ConnectionContext;

#[derive(Clone)]
pub(super) struct ForwardHttpExchangeConnector {
    pub(super) connect_authority: String,
    pub(super) host_header: String,
    pub(super) connect_timeout: Duration,
    pub(super) write_timeout: Duration,
    pub(super) read_timeout: Duration,
    pub(super) limits: MessageLimits,
}

impl fmt::Debug for ForwardHttpExchangeConnector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ForwardHttpExchangeConnector")
            .field("connect_authority", &self.connect_authority)
            .field("host_header", &self.host_header)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl UpstreamConnector for ForwardHttpExchangeConnector {
    async fn send(
        &self,
        _context: &ConnectionContext,
        _ports: &dyn PipelinePorts,
        request: ForwardRequest,
        actions: &[FaultAction],
        write_receipt: UpstreamWriteReceipt,
        informational: Option<&InformationalResponseSink>,
        cancellation: &CancellationToken,
    ) -> Result<UpstreamExchange> {
        let mut headers = request.message.header_map()?;
        strip_hop_by_hop_headers(&mut headers);
        if !headers.contains_key(HOST) {
            headers.insert(
                HOST,
                HeaderValue::from_str(&self.host_header).map_err(|error| {
                    ProxyError::new(ErrorCode::ConfigInvalid, error.to_string())
                })?,
            );
        }
        let mut message = Message::request(
            &request.method,
            &request.uri,
            &headers,
            request.message.body,
        );
        message.set_content_length(message.body.len());
        let mut exchange = crate::http::send_http1_exchange(
            async {
                connect_target(&self.connect_authority, self.connect_timeout, cancellation)
                    .await
                    .map(|stream| Box::new(stream) as crate::transport::BoxIo)
            },
            ForwardRequest { message, ..request },
            actions,
            crate::http::Http1TransportSettings {
                write_timeout: self.write_timeout,
                read_timeout: self.read_timeout,
                limits: self.limits,
            },
            informational,
            &write_receipt,
            cancellation,
        )
        .await?;
        // Forward proxy hop-header filtering remains an adapter policy.
        let response = exchange.final_response;
        let mut headers = response.header_map()?;
        strip_hop_by_hop_headers(&mut headers);
        let status = response
            .http_status()
            .and_then(|status| http::StatusCode::from_u16(status).ok())
            .ok_or_else(|| ProxyError::new(ErrorCode::Io, "upstream response status is invalid"))?;
        exchange.final_response = Message::response(status, &headers, response.body);
        exchange.final_response.validate(self.limits)?;
        Ok(exchange)
    }
}
