//! 外部软件包驱动的 Socket Pipeline capability。
//!
//! 外部包 wire contract 保持 `frame/decode/display/encode` 四个 RPC 不变；每个 RPC 只实现
//! 一个 Exchange trait。唯一写出阶段 Rules 位于 Writer Pipeline，Display 位于 Reader Pipeline。

use std::sync::Arc;

#[cfg(test)]
use intercept_proxy_application::ExternalPackageCallStage;
use intercept_proxy_domain::ProtocolDirection;
use intercept_proxy_exchange::{Direction, Downstream, Upstream};
use intercept_proxy_runtime::{
    PipelinePorts, SocketConnectionIdentity, SocketDirectionCapabilities,
    SocketObservationMetadata, SocketProcessingFailure, SocketProtocolCapabilityFactory,
};

use super::DocumentProgramFactory;
use capabilities::{ExternalDecode, ExternalDisplay, ExternalFrame};

mod capabilities;
pub(super) mod contract;
mod diagnostics;
mod joint_socket;
#[cfg(test)]
pub(crate) use crate::adapters::ProtocolPackageRuntime as ExternalPackageRpc;
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
    rules: DocumentProgramFactory,
    observation: SocketObservationMetadata,
    pipeline: Arc<dyn PipelinePorts>,
    listener_transaction: Arc<tokio::sync::Mutex<()>>,
}

impl ExternalSocketCapabilityFactoryAdapter {
    pub(super) fn new_with_pipeline(
        snapshot: &ExternalSocketRuntimeSnapshot,
        observation: SocketObservationMetadata,
        pipeline: Arc<dyn PipelinePorts>,
    ) -> Self {
        Self {
            binding: snapshot.binding.clone(),
            rules: snapshot.rules.clone(),
            observation,
            pipeline,
            listener_transaction: Arc::clone(&snapshot.listener_transaction),
        }
    }

    fn build<D: Direction>(
        &self,
        connection: &SocketConnectionIdentity,
        binding: DirectionBinding,
    ) -> SocketDirectionCapabilities<D> {
        let methods = binding.methods();
        let runtime = Arc::clone(&self.binding.runtime);
        let package = self.binding.registration.package().identity().clone();
        let observed = Arc::new(parking_lot::Mutex::new(None));
        let prepared = Arc::new(parking_lot::Mutex::new(None));
        let decode = ExternalDecode::<D>::new(
            Arc::clone(&runtime),
            methods.decode,
            package.clone(),
            connection.clone(),
            binding.protocol_direction,
            Arc::clone(&observed),
        );
        let (rules, encode): (
            Box<dyn intercept_proxy_exchange::Rules>,
            Box<dyn intercept_proxy_exchange::Encode<intercept_proxy_exchange::Socket, D>>,
        ) = (
            Box::new(joint_socket::JointSocketRules::new(
                Arc::clone(&self.pipeline),
                connection.clone(),
                self.observation.listener_id.clone(),
                Arc::clone(&runtime),
                binding.protocol_direction,
                Arc::clone(&observed),
                Arc::clone(&prepared),
                self.rules.clone(),
                package.clone(),
                Arc::clone(&self.listener_transaction),
            )),
            Box::new(joint_socket::PreparedSocketEncode::<D>::new(prepared)),
        );
        SocketDirectionCapabilities::new(
            Box::new(ExternalFrame::<D>::new(
                Arc::clone(&runtime),
                methods.frame,
                package.clone(),
                connection.clone(),
                binding.protocol_direction,
            )),
            Box::new(decode),
            Box::new(ExternalDisplay::new(
                Arc::clone(&self.binding.runtime),
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
            },
            ProtocolDirection::Downstream => DirectionBinding {
                protocol_direction: ProtocolDirection::Downstream,
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
}

impl DirectionBinding {
    fn methods(self) -> DirectionMethods {
        match self.protocol_direction {
            ProtocolDirection::Upstream => DirectionMethods {
                frame: "hooks.upstream.frame",
                decode: "hooks.upstream.decode",
                display: "document.upstream.display",
            },
            ProtocolDirection::Downstream => DirectionMethods {
                frame: "hooks.downstream.frame",
                decode: "hooks.downstream.decode",
                display: "document.downstream.display",
            },
        }
    }
}

struct DirectionMethods {
    frame: &'static str,
    decode: &'static str,
    display: &'static str,
}

#[cfg(test)]
mod tests;
