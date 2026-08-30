//! 外部软件包驱动的 Socket Pipeline capability。
//!
//! 外部包 wire contract 保持 `frame/decode/display/encode` 四个 RPC 不变；每个 RPC 只实现
//! 一个 Exchange trait。两段宿主 Rules 位于 Writer Pipeline，Display 位于 Reader Pipeline。

use std::sync::Arc;

#[cfg(test)]
use intercept_proxy_application::ExternalPackageCallStage;
use intercept_proxy_domain::ProtocolDirection;
#[cfg(test)]
use intercept_proxy_domain::ProtocolRuleStage;
use intercept_proxy_exchange::{Direction, Downstream, Upstream};
use intercept_proxy_runtime::{
    PipelinePorts, SocketConnectionIdentity, SocketDirectionCapabilities,
    SocketObservationMetadata, SocketProcessingFailure, SocketProtocolCapabilityFactory,
};

use super::ProtocolDocumentRuleConnectionFactory;
use capabilities::{ExternalDecode, ExternalDisplay, ExternalFrame};
#[cfg(test)]
use capabilities::{ExternalEncode, OrderedRules};

mod capabilities;
pub(super) mod contract;
mod diagnostics;
mod joint_socket;
pub(crate) use contract::ExternalPackageRpc;
pub(crate) use contract::ExternalSocketPackageBinding as RuntimeExternalSocketPackageBinding;
use contract::ExternalSocketPackageBinding;
pub(crate) use contract::ExternalSocketPackageProvider;
pub(crate) use contract::ExternalSocketRuntimeSnapshot;
#[cfg(test)]
use diagnostics::redacted_data_summary;
pub(super) use diagnostics::trace_external_rpc_failure;

/// 同一次 Listener 启动快照派生的双方向 capability factory。
pub(super) struct ExternalSocketCapabilityFactoryAdapter {
    binding: ExternalSocketPackageBinding,
    rules: ProtocolDocumentRuleConnectionFactory,
    observation: SocketObservationMetadata,
    pipeline: Option<Arc<dyn PipelinePorts>>,
}

impl ExternalSocketCapabilityFactoryAdapter {
    #[cfg(test)]
    pub(super) fn new(
        snapshot: &ExternalSocketRuntimeSnapshot,
        observation: SocketObservationMetadata,
    ) -> Self {
        Self {
            binding: snapshot.binding.clone(),
            rules: snapshot.rules.clone(),
            observation,
            pipeline: None,
        }
    }

    pub(super) fn new_with_pipeline(
        snapshot: &ExternalSocketRuntimeSnapshot,
        observation: SocketObservationMetadata,
        pipeline: Arc<dyn PipelinePorts>,
    ) -> Self {
        Self {
            binding: snapshot.binding.clone(),
            rules: snapshot.rules.clone(),
            observation,
            pipeline: Some(pipeline),
        }
    }

    fn build<D: Direction>(
        &self,
        connection: &SocketConnectionIdentity,
        binding: DirectionBinding,
    ) -> SocketDirectionCapabilities<D> {
        let methods = binding.methods();
        let rpc = Arc::clone(&self.binding.rpc);
        let package = self.binding.registration.package().identity().clone();
        let observed = Arc::new(parking_lot::Mutex::new(None));
        let prepared = Arc::new(parking_lot::Mutex::new(None));
        let decode = ExternalDecode::<D>::new(
            Arc::clone(&rpc),
            methods.decode,
            package.clone(),
            connection.clone(),
            binding.protocol_direction,
            Arc::clone(&observed),
        );
        let (rules, encode): (
            Box<dyn intercept_proxy_exchange::Rules>,
            Box<dyn intercept_proxy_exchange::Encode<intercept_proxy_exchange::Socket, D>>,
        ) = if let Some(pipeline) = &self.pipeline {
            (
                Box::new(joint_socket::JointSocketRules::new(
                    Arc::clone(pipeline),
                    connection.clone(),
                    self.observation.listener_id.clone(),
                    Arc::clone(&rpc),
                    binding.protocol_direction,
                    Arc::clone(&observed),
                    Arc::clone(&prepared),
                    self.rules.direction_programs(binding.protocol_direction),
                    package.clone(),
                )),
                Box::new(joint_socket::PreparedSocketEncode::<D>::new(prepared)),
            )
        } else {
            #[cfg(test)]
            {
                (
                    Box::new(OrderedRules::<D>::new(
                        self.rules
                            .connection(connection.clone(), binding.first_rules),
                        self.rules
                            .connection(connection.clone(), binding.second_rules),
                    )),
                    Box::new(ExternalEncode::<D>::new(
                        Arc::clone(&rpc),
                        methods.encode,
                        package.clone(),
                        connection.clone(),
                        binding.protocol_direction,
                    )),
                )
            }
            #[cfg(not(test))]
            unreachable!("production external Socket factory always has pipeline ports")
        };
        SocketDirectionCapabilities::new(
            Box::new(ExternalFrame::<D>::new(
                Arc::clone(&rpc),
                methods.frame,
                package.clone(),
                connection.clone(),
                binding.protocol_direction,
            )),
            Box::new(decode),
            Box::new(ExternalDisplay::new(
                Arc::clone(&self.binding.rpc),
                methods.display,
                package.clone(),
                connection.clone(),
                binding.protocol_direction,
            )),
            rules,
            encode,
        )
    }

    fn direction(direction: ProtocolDirection) -> DirectionBinding {
        match direction {
            ProtocolDirection::Upstream => DirectionBinding {
                protocol_direction: ProtocolDirection::Upstream,
                #[cfg(test)]
                first_rules: ProtocolRuleStage::AppToProxy,
                #[cfg(test)]
                second_rules: ProtocolRuleStage::ProxyToUpstream,
            },
            ProtocolDirection::Downstream => DirectionBinding {
                protocol_direction: ProtocolDirection::Downstream,
                #[cfg(test)]
                first_rules: ProtocolRuleStage::UpstreamToProxy,
                #[cfg(test)]
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
        Ok(self.build(&connection, Self::direction(ProtocolDirection::Upstream)))
    }

    fn create_downstream(
        &self,
        connection: SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Downstream>, SocketProcessingFailure> {
        Ok(self.build(&connection, Self::direction(ProtocolDirection::Downstream)))
    }
}

#[derive(Clone, Copy)]
struct DirectionBinding {
    protocol_direction: ProtocolDirection,
    #[cfg(test)]
    first_rules: ProtocolRuleStage,
    #[cfg(test)]
    second_rules: ProtocolRuleStage,
}

impl DirectionBinding {
    fn methods(self) -> DirectionMethods {
        match self.protocol_direction {
            ProtocolDirection::Upstream => DirectionMethods {
                frame: "hooks.upstream.frame",
                decode: "hooks.upstream.decode",
                #[cfg(test)]
                encode: "hooks.upstream.encode",
                display: "document.upstream.display",
            },
            ProtocolDirection::Downstream => DirectionMethods {
                frame: "hooks.downstream.frame",
                decode: "hooks.downstream.decode",
                #[cfg(test)]
                encode: "hooks.downstream.encode",
                display: "document.downstream.display",
            },
        }
    }
}

struct DirectionMethods {
    frame: &'static str,
    decode: &'static str,
    #[cfg(test)]
    encode: &'static str,
    display: &'static str,
}

#[cfg(test)]
mod tests;
