use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use intercept_proxy_domain::{
    Condition, ConditionTree, Document, ListenerId, ProtocolDirection, ProtocolPackageId,
    ProtocolPackageRef, ProtocolPackageVersion, RuleId, RuleProgramEntry, UnifiedAction,
    UnifiedRuleProgram,
};
use intercept_proxy_exchange::{Rules, SocketContext};
use intercept_proxy_package_contract::{
    DecodeParams, DisplayParams, EncodeParams, FrameParams, FrameResult,
};
use intercept_proxy_runtime::{
    ConnectionContext, HandshakePolicy, JointRuleConditionEvaluation, PipelinePorts,
    Result as ProxyResult, SocketJointEvaluation, SocketPayloadDirection,
};
use parking_lot::Mutex;
use uuid::Uuid;

use super::{ExternalSocketObserved, JointSocketRules};
use crate::adapters::PackageTransportError;
use crate::adapters::listener_runtime::{
    DocumentProgramFactory, external_relay::ExternalPackageRpc,
};

#[derive(Debug)]
struct UnusedRpc;

#[async_trait]
impl ExternalPackageRpc for UnusedRpc {
    async fn frame(
        &self,
        _: ProtocolDirection,
        _: FrameParams,
    ) -> Result<FrameResult, PackageTransportError> {
        unreachable!("generation test never calls package RPC")
    }

    async fn decode(
        &self,
        _: ProtocolDirection,
        _: DecodeParams,
    ) -> Result<Document, PackageTransportError> {
        unreachable!("generation test never calls package RPC")
    }

    async fn encode(
        &self,
        _: ProtocolDirection,
        _: EncodeParams,
    ) -> Result<String, PackageTransportError> {
        unreachable!("generation test never calls package RPC")
    }

    async fn display(
        &self,
        _: ProtocolDirection,
        _: DisplayParams,
    ) -> Result<String, PackageTransportError> {
        unreachable!("generation test never calls package RPC")
    }
}

#[derive(Debug)]
struct GenerationRecordingPipeline {
    expected_rule: RuleId,
    observed_new_generation: AtomicBool,
}

impl HandshakePolicy for GenerationRecordingPipeline {}

#[async_trait]
impl PipelinePorts for GenerationRecordingPipeline {
    async fn apply_socket_policy(
        &self,
        _: &ConnectionContext,
        _: SocketPayloadDirection,
        mut evaluation: Box<dyn SocketJointEvaluation>,
    ) -> ProxyResult<SocketContext> {
        self.observed_new_generation.store(
            matches!(
                evaluation.gate(self.expected_rule.as_uuid(), 1)?,
                JointRuleConditionEvaluation::UnifiedOwned(_)
            ),
            Ordering::Release,
        );
        Ok(SocketContext {
            data: b"new".to_vec(),
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn socket_frame_waits_for_listener_gate_before_reading_rule_generation() {
    let listener_id = ListenerId::from_uuid(Uuid::new_v4());
    let package = ProtocolPackageRef {
        id: ProtocolPackageId::new("socket-generation").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    };
    let empty = Arc::new(UnifiedRuleProgram::new(Vec::new()).unwrap());
    let programs =
        DocumentProgramFactory::new(listener_id, package.clone(), Arc::clone(&empty), empty);
    let expected_rule = RuleId::new();
    let replacement = DocumentProgramFactory::new(
        listener_id,
        package.clone(),
        Arc::new(
            UnifiedRuleProgram::new(vec![
                RuleProgramEntry::new(
                    expected_rule,
                    1,
                    1,
                    ConditionTree::Leaf(Condition::NthHit { count: 1 }),
                    vec![UnifiedAction::RecordMatch],
                )
                .unwrap(),
            ])
            .unwrap(),
        ),
        Arc::new(UnifiedRuleProgram::new(Vec::new()).unwrap()),
    );
    let pipeline = Arc::new(GenerationRecordingPipeline {
        expected_rule,
        observed_new_generation: AtomicBool::new(false),
    });
    let transaction = Arc::new(tokio::sync::Mutex::new(()));
    let transaction_guard = Arc::clone(&transaction).lock_owned().await;
    let document: Document = serde_json::from_value(serde_json::json!({})).unwrap();
    let mut rules = JointSocketRules::new(
        pipeline.clone(),
        intercept_proxy_runtime::SocketConnectionIdentity {
            runtime_epoch: Uuid::new_v4(),
            connection_id: Uuid::new_v4(),
            peer_addr: "127.0.0.1:12345".parse().unwrap(),
        },
        listener_id.to_string(),
        Arc::new(UnusedRpc),
        ProtocolDirection::Upstream,
        Arc::new(Mutex::new(Some(ExternalSocketObserved {
            document: document.clone(),
            input: b"old".to_vec(),
        }))),
        Arc::new(Mutex::new(None)),
        programs.clone(),
        package,
        transaction,
    );
    let mut applying = Box::pin(rules.apply(document));
    assert!(matches!(
        std::future::poll_fn(|context| { std::task::Poll::Ready(applying.as_mut().poll(context)) })
            .await,
        std::task::Poll::Pending
    ));

    programs.replace(&replacement);
    drop(transaction_guard);
    applying.await.unwrap();

    assert!(pipeline.observed_new_generation.load(Ordering::Acquire));
}
