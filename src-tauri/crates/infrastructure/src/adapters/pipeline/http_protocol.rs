use intercept_proxy_application::{HttpProtocolBodyViewModel, HttpProtocolFailureViewModel};
use intercept_proxy_domain::MessageStage;
use intercept_proxy_protocol_scripting::ProtocolDirection;
use intercept_proxy_runtime::{ConnectionContext, Message, Result as ProxyResult};

use crate::adapters::listener_runtime::HttpProtocolObservationSink;

use super::{RuntimePipelineAdapter, content_view};

impl HttpProtocolObservationSink for RuntimePipelineAdapter {
    fn record_http_protocol_observation(
        &self,
        context: &ConnectionContext,
        direction: ProtocolDirection,
        message: &Message,
        observation: HttpProtocolBodyViewModel,
    ) -> ProxyResult<()> {
        let stage = match direction {
            ProtocolDirection::Upstream => MessageStage::Request,
            ProtocolDirection::Downstream => MessageStage::Response,
        };
        let body_codec = self.codec_for(context, stage, message)?;
        let mut message_content = content_view(body_codec.as_ref(), message);
        message_content.protocol = Some(observation);
        message_content.protocol_failure = None;
        self.update_live_session(context, move |record| {
            let summary = &mut record.detail.summary;
            summary.revision = summary.revision.saturating_add(1);
            match direction {
                ProtocolDirection::Upstream => {
                    summary.request_size_bytes = message.body.len() as u64;
                    record.detail.request = Some(message_content);
                }
                ProtocolDirection::Downstream => {
                    summary.response_size_bytes = message.body.len() as u64;
                    summary.http_status = message.http_status();
                    record.detail.response = Some(message_content);
                }
            }
        })?;
        Ok(())
    }

    fn record_http_protocol_failure(
        &self,
        context: &ConnectionContext,
        message: &Message,
        failure: HttpProtocolFailureViewModel,
    ) -> ProxyResult<()> {
        let direction = failure.direction;
        let stage = match direction {
            intercept_proxy_domain::ProtocolDirection::Upstream => MessageStage::Request,
            intercept_proxy_domain::ProtocolDirection::Downstream => MessageStage::Response,
        };
        let body_codec = self.codec_for(context, stage, message)?;
        let mut message_content = content_view(body_codec.as_ref(), message);
        message_content.protocol = None;
        message_content.protocol_failure = Some(failure);
        self.update_live_session(context, move |record| {
            let summary = &mut record.detail.summary;
            summary.revision = summary.revision.saturating_add(1);
            match direction {
                intercept_proxy_domain::ProtocolDirection::Upstream => {
                    summary.request_size_bytes = message.body.len() as u64;
                    record.detail.request = Some(message_content);
                }
                intercept_proxy_domain::ProtocolDirection::Downstream => {
                    summary.response_size_bytes = message.body.len() as u64;
                    summary.http_status = message.http_status();
                    record.detail.response = Some(message_content);
                }
            }
        })?;
        Ok(())
    }
}
