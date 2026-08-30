//! 外部软件包驱动的 Socket Pipeline capability。
//!
//! 外部包 wire contract 保持 `frame/decode/display/encode` 四个 RPC 不变；每个 RPC 只实现
//! 一个 Exchange trait。两段宿主 Rules 位于 Writer Pipeline，Display 位于 Reader Pipeline。

use std::sync::Arc;

#[cfg(test)]
use intercept_proxy_application::ExternalPackageCallStage;
use intercept_proxy_domain::{ProtocolDirection, ProtocolRuleStage};
use intercept_proxy_exchange::{Direction, Downstream, Upstream};
use intercept_proxy_runtime::{
    SocketConnectionIdentity, SocketDirectionCapabilities, SocketObservationMetadata,
    SocketProcessingFailure, SocketProtocolCapabilityFactory,
};

use super::ProtocolDocumentRuleConnectionFactory;
use capabilities::{ExternalDecode, ExternalDisplay, ExternalEncode, ExternalFrame, OrderedRules};

mod capabilities;
pub(super) mod contract;
mod diagnostics;
pub(crate) use contract::ExternalSocketPackageBinding as RuntimeExternalSocketPackageBinding;
pub(crate) use contract::ExternalSocketPackageProvider;
pub(crate) use contract::ExternalSocketRuntimeSnapshot;
use contract::{ExternalPackageRpc, ExternalSocketPackageBinding};
#[cfg(test)]
use diagnostics::redacted_data_summary;
pub(super) use diagnostics::trace_external_rpc_failure;

/// 同一次 Listener 启动快照派生的双方向 capability factory。
pub(super) struct ExternalSocketCapabilityFactoryAdapter {
    binding: ExternalSocketPackageBinding,
    rules: ProtocolDocumentRuleConnectionFactory,
    observation: SocketObservationMetadata,
}

impl ExternalSocketCapabilityFactoryAdapter {
    pub(super) fn new(
        snapshot: &ExternalSocketRuntimeSnapshot,
        observation: SocketObservationMetadata,
    ) -> Self {
        Self {
            binding: snapshot.binding.clone(),
            rules: snapshot.rules.clone(),
            observation,
        }
    }

    fn build<D: Direction>(
        &self,
        connection: SocketConnectionIdentity,
        binding: DirectionBinding,
    ) -> SocketDirectionCapabilities<D> {
        let package = self.binding.registration.package().identity().clone();
        let methods = binding.methods();
        SocketDirectionCapabilities::new(
            Box::new(ExternalFrame::<D>::new(
                Arc::clone(&self.binding.rpc),
                methods.frame,
                package.clone(),
                connection.clone(),
                binding.protocol_direction,
            )),
            Box::new(ExternalDecode::<D>::new(
                Arc::clone(&self.binding.rpc),
                methods.decode,
                package.clone(),
                connection.clone(),
                binding.protocol_direction,
            )),
            Box::new(ExternalDisplay::new(
                Arc::clone(&self.binding.rpc),
                methods.display,
                package.clone(),
                connection.clone(),
                binding.protocol_direction,
            )),
            Box::new(OrderedRules::<D>::new(
                self.rules
                    .connection(connection.clone(), binding.first_rules),
                self.rules
                    .connection(connection.clone(), binding.second_rules),
            )),
            Box::new(ExternalEncode::<D>::new(
                Arc::clone(&self.binding.rpc),
                methods.encode,
                package,
                connection,
                binding.protocol_direction,
            )),
        )
    }

    fn direction(direction: ProtocolDirection) -> DirectionBinding {
        match direction {
            ProtocolDirection::Upstream => DirectionBinding {
                protocol_direction: ProtocolDirection::Upstream,
                first_rules: ProtocolRuleStage::AppToProxy,
                second_rules: ProtocolRuleStage::ProxyToUpstream,
            },
            ProtocolDirection::Downstream => DirectionBinding {
                protocol_direction: ProtocolDirection::Downstream,
                first_rules: ProtocolRuleStage::UpstreamToProxy,
                second_rules: ProtocolRuleStage::ProxyToApp,
            },
        }
    }
}

impl SocketProtocolCapabilityFactory for ExternalSocketCapabilityFactoryAdapter {
    fn observation_metadata(&self) -> SocketObservationMetadata {
        self.observation.clone()
    }

    fn create_upstream(
        &self,
        connection: SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Upstream>, SocketProcessingFailure> {
        Ok(self.build(connection, Self::direction(ProtocolDirection::Upstream)))
    }

    fn create_downstream(
        &self,
        connection: SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Downstream>, SocketProcessingFailure> {
        Ok(self.build(connection, Self::direction(ProtocolDirection::Downstream)))
    }
}

#[derive(Clone, Copy)]
struct DirectionBinding {
    protocol_direction: ProtocolDirection,
    first_rules: ProtocolRuleStage,
    second_rules: ProtocolRuleStage,
}

impl DirectionBinding {
    fn methods(self) -> DirectionMethods {
        match self.protocol_direction {
            ProtocolDirection::Upstream => DirectionMethods {
                frame: "hooks.upstream.frame",
                decode: "hooks.upstream.decode",
                encode: "hooks.upstream.encode",
                display: "document.upstream.display",
            },
            ProtocolDirection::Downstream => DirectionMethods {
                frame: "hooks.downstream.frame",
                decode: "hooks.downstream.decode",
                encode: "hooks.downstream.encode",
                display: "document.downstream.display",
            },
        }
    }
}

struct DirectionMethods {
    frame: &'static str,
    decode: &'static str,
    encode: &'static str,
    display: &'static str,
}

#[cfg(test)]
mod tests;
