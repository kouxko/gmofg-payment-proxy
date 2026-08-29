//! 外部软件包驱动的 Socket Pipeline capability。
//!
//! 外部包 wire contract 保持 `frame/decode/display/encode` 四个 RPC 不变；每个 RPC 只实现
//! 一个 Exchange trait。两段宿主 Rules 位于 Writer Pipeline，Display 位于 Reader Pipeline。

use std::sync::Arc;

#[cfg(test)]
use intercept_proxy_application::ExternalPackageCallStage;
use intercept_proxy_domain::{
    ExternalPackageDirection, ExternalPackageMethodNamespace, ProtocolDirection, ProtocolRuleStage,
};
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
        binding: DirectionBinding<'_>,
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

    fn direction(&self, direction: ExternalPackageDirection) -> DirectionBinding<'_> {
        let registration = &self.binding.registration;
        match direction {
            ExternalPackageDirection::Upstream => DirectionBinding {
                external_direction: direction,
                protocol_direction: ProtocolDirection::Upstream,
                first_rules: ProtocolRuleStage::AppToProxy,
                second_rules: ProtocolRuleStage::ProxyToUpstream,
                hooks: registration.hooks().upstream(),
                document: registration.document().upstream(),
            },
            ExternalPackageDirection::Downstream => DirectionBinding {
                external_direction: direction,
                protocol_direction: ProtocolDirection::Downstream,
                first_rules: ProtocolRuleStage::UpstreamToProxy,
                second_rules: ProtocolRuleStage::ProxyToApp,
                hooks: registration.hooks().downstream(),
                document: registration.document().downstream(),
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
        Ok(self.build(
            connection,
            self.direction(ExternalPackageDirection::Upstream),
        ))
    }

    fn create_downstream(
        &self,
        connection: SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Downstream>, SocketProcessingFailure> {
        Ok(self.build(
            connection,
            self.direction(ExternalPackageDirection::Downstream),
        ))
    }
}

#[derive(Clone, Copy)]
struct DirectionBinding<'a> {
    external_direction: ExternalPackageDirection,
    protocol_direction: ProtocolDirection,
    first_rules: ProtocolRuleStage,
    second_rules: ProtocolRuleStage,
    hooks: &'a intercept_proxy_domain::ExternalPackageDirectionHooks,
    document: &'a intercept_proxy_domain::ExternalPackageDocumentDirection,
}

impl DirectionBinding<'_> {
    fn methods(&self) -> DirectionMethods {
        DirectionMethods {
            frame: self.hooks.frame().qualified(
                ExternalPackageMethodNamespace::Hooks,
                self.external_direction,
            ),
            decode: self.hooks.decode().qualified(
                ExternalPackageMethodNamespace::Hooks,
                self.external_direction,
            ),
            encode: self.hooks.encode().qualified(
                ExternalPackageMethodNamespace::Hooks,
                self.external_direction,
            ),
            display: self.document.display().qualified(
                ExternalPackageMethodNamespace::Document,
                self.external_direction,
            ),
        }
    }
}

struct DirectionMethods {
    frame: String,
    decode: String,
    encode: String,
    display: String,
}

#[cfg(test)]
mod tests;
