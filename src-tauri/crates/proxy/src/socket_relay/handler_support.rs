use std::{net::SocketAddr, sync::Arc};

use uuid::Uuid;

use crate::transport::relay::{RelayFailure, RelayOperation};
use crate::{ErrorCode, ProxyError};

use super::{
    SocketConnectionIdentity, SocketConnectionTarget, SocketLocalResponderConfig,
    SocketPayloadDirection, SocketPipelineLimits, SocketProcessingFailure,
    SocketProcessingFailureKind, SocketProtocolCapabilityFactory, SocketRelayConfig,
    SocketRelayDirection, SocketRelayFailure, SocketRelayRunContext, SocketRelayStage,
};

#[derive(Clone, Debug)]
pub(super) enum SocketHandlerConfig {
    Relay(SocketRelayConfig),
    LocalResponder(SocketLocalResponderConfig),
}

impl SocketHandlerConfig {
    pub(super) fn target(&self) -> SocketConnectionTarget {
        match self {
            Self::Relay(config) => SocketConnectionTarget::Relay(format!(
                "{}:{}",
                config.upstream.host, config.upstream.port
            )),
            Self::LocalResponder(_) => SocketConnectionTarget::LocalResponder,
        }
    }
}

pub(super) enum SocketHandlerProcessing {
    Direct,
    DirectLocal,
    ScriptedRelay {
        factory: Arc<dyn SocketProtocolCapabilityFactory>,
        limits: SocketPipelineLimits,
    },
    LocalResponder {
        factory: Arc<dyn SocketProtocolCapabilityFactory>,
        limits: SocketPipelineLimits,
    },
}

impl std::fmt::Debug for SocketHandlerProcessing {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Direct => "Direct",
            Self::DirectLocal => "DirectLocal",
            Self::ScriptedRelay { .. } => "ScriptedRelay",
            Self::LocalResponder { .. } => "LocalResponder",
        })
    }
}

pub(super) fn connection_identity(
    run: &SocketRelayRunContext,
    connection_id: Uuid,
    peer_addr: SocketAddr,
) -> SocketConnectionIdentity {
    SocketConnectionIdentity {
        runtime_epoch: run.listener_run_epoch,
        connection_id,
        peer_addr,
    }
}

pub(super) fn socket_failure(failure: &RelayFailure) -> SocketRelayFailure {
    let direction = match failure.direction {
        crate::transport::relay::RelayDirection::ClientToServer => {
            SocketRelayDirection::ClientToServer
        }
        crate::transport::relay::RelayDirection::ServerToClient => {
            SocketRelayDirection::ServerToClient
        }
    };
    if failure.error.code == ErrorCode::ProxyStopped.as_str() {
        return SocketRelayFailure {
            stage: SocketRelayStage::Shutdown,
            direction: Some(direction),
            code: ErrorCode::SocketRelayCancelled.as_str(),
            external_package_call: None,
        };
    }
    let (stage, code) = match failure.operation {
        RelayOperation::Read => (
            SocketRelayStage::RelayRead,
            if failure.error.code == ErrorCode::SocketReadTimeout.as_str() {
                ErrorCode::SocketReadTimeout.as_str()
            } else {
                ErrorCode::SocketReadFailed.as_str()
            },
        ),
        RelayOperation::Write | RelayOperation::Flush | RelayOperation::HalfClose => (
            SocketRelayStage::RelayWrite,
            if failure.error.code == ErrorCode::SocketWriteTimeout.as_str() {
                ErrorCode::SocketWriteTimeout.as_str()
            } else {
                ErrorCode::SocketWriteFailed.as_str()
            },
        ),
    };
    SocketRelayFailure {
        stage,
        direction: Some(direction),
        code,
        external_package_call: None,
    }
}

pub(super) fn processing_failure(failure: &SocketProcessingFailure) -> SocketRelayFailure {
    let direction = failure.direction.map(|direction| match direction {
        SocketPayloadDirection::AppToUpstream => SocketRelayDirection::ClientToServer,
        SocketPayloadDirection::UpstreamToApp => SocketRelayDirection::ServerToClient,
        SocketPayloadDirection::LocalExchange => SocketRelayDirection::LocalExchange,
    });
    let stage = match failure.kind {
        SocketProcessingFailureKind::InvalidLimits
        | SocketProcessingFailureKind::InvalidFrameBoundary
        | SocketProcessingFailureKind::FrameRejected
        | SocketProcessingFailureKind::BufferLimitExceeded
        | SocketProcessingFailureKind::TruncatedFrame => SocketRelayStage::FrameInspect,
        SocketProcessingFailureKind::ProcessingFailed
        | SocketProcessingFailureKind::ProcessingTimeout
        | SocketProcessingFailureKind::ProcessorPanicked
        | SocketProcessingFailureKind::EmptyOutput
        | SocketProcessingFailureKind::OutputLimitExceeded => SocketRelayStage::FrameProcess,
        SocketProcessingFailureKind::ReadFailed | SocketProcessingFailureKind::ReadTimeout => {
            SocketRelayStage::RelayRead
        }
        SocketProcessingFailureKind::DecodeFailed => SocketRelayStage::Decode,
        SocketProcessingFailureKind::RuleFailed => SocketRelayStage::Rule,
        SocketProcessingFailureKind::EncodeFailed => SocketRelayStage::Encode,
        SocketProcessingFailureKind::WriteFailed | SocketProcessingFailureKind::WriteTimeout => {
            SocketRelayStage::RelayWrite
        }
        SocketProcessingFailureKind::Cancelled => SocketRelayStage::Shutdown,
    };
    SocketRelayFailure {
        stage,
        direction,
        code: if failure.kind == SocketProcessingFailureKind::Cancelled {
            ErrorCode::SocketRelayCancelled.as_str()
        } else {
            failure.stable_code()
        },
        external_package_call: failure.external_package_call.clone(),
    }
}

pub(super) fn normalize_cancelled(error: ProxyError) -> ProxyError {
    if error.code == ErrorCode::ProxyStopped.as_str() {
        ProxyError::new(ErrorCode::SocketRelayCancelled, error.message)
    } else {
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_processing_kinds_keep_distinct_terminal_stages() {
        for (kind, expected_stage) in [
            (
                SocketProcessingFailureKind::DecodeFailed,
                SocketRelayStage::Decode,
            ),
            (
                SocketProcessingFailureKind::RuleFailed,
                SocketRelayStage::Rule,
            ),
            (
                SocketProcessingFailureKind::EncodeFailed,
                SocketRelayStage::Encode,
            ),
        ] {
            let mapped = processing_failure(&SocketProcessingFailure::new(kind, "private detail"));
            assert_eq!(mapped.stage, expected_stage);
            assert_eq!(mapped.code, kind.as_str());
        }
    }
}
