//! Protocol-neutral joint Document evaluation and lifecycle transaction state.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use intercept_proxy_domain::{
    Document, ProtocolDirection, ProtocolDocumentRuleId, ProtocolDocumentRuleProgram,
    ProtocolPackageRef, Rule, RuleId,
};
use intercept_proxy_exchange::{
    Error, ExternalPackageCallFailure, ExternalPackageCallStage, SocketContext,
};
use intercept_proxy_package_contract::{CanonicalBase64, EncodeParams};
use intercept_proxy_product_api::BodyCodec;
use intercept_proxy_runtime::{ConnectionContext, Message, SocketJointEvaluation};
use parking_lot::Mutex;
use uuid::Uuid;

use super::external_relay::ExternalPackageRpc;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct PendingKey {
    runtime_epoch: Uuid,
    connection_id: Uuid,
    response: bool,
}

#[derive(Debug, Default)]
pub(crate) struct JointHttpRuleRuntime {
    pending: Mutex<HashMap<PendingKey, JointDocumentEvaluation>>,
}

impl JointHttpRuleRuntime {
    pub(super) fn stage(
        &self,
        runtime_epoch: Uuid,
        connection_id: Uuid,
        response: bool,
        evaluation: JointDocumentEvaluation,
    ) {
        self.pending.lock().insert(
            PendingKey {
                runtime_epoch,
                connection_id,
                response,
            },
            evaluation,
        );
    }

    pub(crate) fn take(
        &self,
        context: &ConnectionContext,
        response: bool,
    ) -> Option<JointDocumentEvaluation> {
        self.take_identity(context.runtime_epoch, context.connection_id, response)
    }

    pub(super) fn take_identity(
        &self,
        runtime_epoch: Uuid,
        connection_id: Uuid,
        response: bool,
    ) -> Option<JointDocumentEvaluation> {
        self.pending.lock().remove(&PendingKey {
            runtime_epoch,
            connection_id,
            response,
        })
    }

    pub(crate) fn remove_connection(&self, context: &ConnectionContext) {
        self.pending.lock().retain(|key, _| {
            key.runtime_epoch != context.runtime_epoch || key.connection_id != context.connection_id
        });
    }
}

#[derive(Clone)]
pub(crate) struct JointDocumentEvaluation {
    document: Document,
    original_document: Document,
    encoder: JointDocumentEncoder,
    programs: HashMap<RuleId, Arc<ProtocolDocumentRuleProgram>>,
}

#[derive(Clone)]
enum JointDocumentEncoder {
    External {
        original_input: String,
        rpc: Arc<dyn ExternalPackageRpc>,
        direction: ProtocolDirection,
        codec: Arc<dyn BodyCodec>,
        package: ProtocolPackageRef,
    },
    SocketExternal {
        original_input: Vec<u8>,
        rpc: Arc<dyn ExternalPackageRpc>,
        direction: ProtocolDirection,
        package: ProtocolPackageRef,
    },
}

impl std::fmt::Debug for JointDocumentEvaluation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JointDocumentEvaluation")
            .field("document", &self.document)
            .field("rule_count", &self.programs.len())
            .finish_non_exhaustive()
    }
}

impl JointDocumentEvaluation {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_external(
        document: Document,
        original_document: Document,
        original_input: String,
        rpc: Arc<dyn ExternalPackageRpc>,
        direction: ProtocolDirection,
        codec: Arc<dyn BodyCodec>,
        package: ProtocolPackageRef,
        programs: impl IntoIterator<Item = Arc<ProtocolDocumentRuleProgram>>,
    ) -> Self {
        let programs = programs
            .into_iter()
            .flat_map(|program| {
                program
                    .rules()
                    .iter()
                    .map(|rule| {
                        (
                            RuleId::from_uuid(rule.rule_id().as_uuid()),
                            Arc::clone(&program),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        Self {
            document,
            original_document,
            encoder: JointDocumentEncoder::External {
                original_input,
                rpc,
                direction,
                codec,
                package,
            },
            programs,
        }
    }

    pub(crate) fn new_external_socket(
        document: Document,
        original_document: Document,
        original_input: Vec<u8>,
        rpc: Arc<dyn ExternalPackageRpc>,
        direction: ProtocolDirection,
        package: ProtocolPackageRef,
        programs: impl IntoIterator<Item = Arc<ProtocolDocumentRuleProgram>>,
    ) -> Self {
        let programs = programs
            .into_iter()
            .flat_map(|program| {
                program
                    .rules()
                    .iter()
                    .map(|rule| {
                        (
                            RuleId::from_uuid(rule.rule_id().as_uuid()),
                            Arc::clone(&program),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        Self {
            document,
            original_document,
            encoder: JointDocumentEncoder::SocketExternal {
                original_input,
                rpc,
                direction,
                package,
            },
            programs,
        }
    }

    pub(crate) fn gate(
        &mut self,
        rule: &Rule,
    ) -> Result<bool, intercept_proxy_domain::DomainError> {
        let Some(program) = self.programs.get(&rule.id) else {
            return Ok(true);
        };
        program.apply_rule_if_matches(
            ProtocolDocumentRuleId::from_uuid(rule.id.as_uuid()),
            &mut self.document,
        )
    }

    pub(crate) async fn encode_into(self, message: &mut Message) -> Result<(), Error> {
        let written = match self.encoder {
            JointDocumentEncoder::External {
                original_input,
                rpc,
                direction,
                codec,
                package,
            } => {
                if self.document == self.original_document {
                    return Ok(());
                }
                let encoded = rpc
                    .encode(
                        direction,
                        EncodeParams {
                            original_input,
                            document: self.document,
                        },
                    )
                    .await
                    .map_err(|error| {
                        external_rpc_error(
                            package,
                            direction,
                            ExternalPackageCallStage::Encode,
                            "hooks.encode",
                            &error,
                        )
                    })?;
                codec
                    .encode(&encoded)
                    .map_err(|error| Error::new(format!("{}: {}", error.code, error.message)))?
            }
            JointDocumentEncoder::SocketExternal { .. } => {
                return Err(Error::new(
                    "SOCKET_JOINT_OUTPUT_MISMATCH: Socket evaluation reached HTTP encoder",
                ));
            }
        };
        message.replace_body(written.into());
        Ok(())
    }

    async fn encode_socket(self) -> Result<SocketContext, Error> {
        let JointDocumentEncoder::SocketExternal {
            original_input,
            rpc,
            direction,
            package,
        } = self.encoder
        else {
            return Err(Error::new(
                "HTTP_JOINT_OUTPUT_MISMATCH: HTTP evaluation reached Socket encoder",
            ));
        };
        if self.document == self.original_document {
            return Ok(SocketContext {
                data: original_input,
            });
        }
        let encoded = rpc
            .encode(
                direction,
                EncodeParams {
                    original_input: CanonicalBase64::from_bytes(&original_input)
                        .as_str()
                        .to_owned(),
                    document: self.document,
                },
            )
            .await
            .map_err(|error| {
                external_rpc_error(
                    package,
                    direction,
                    ExternalPackageCallStage::Encode,
                    "hooks.encode",
                    &error,
                )
            })?;
        let encoded = CanonicalBase64::try_from(encoded).map_err(|_| {
            Error::new("ENCODE_FAILED: Socket package returned non-canonical Base64")
        })?;
        Ok(SocketContext {
            data: encoded.bytes(),
        })
    }
}

#[async_trait]
impl SocketJointEvaluation for JointDocumentEvaluation {
    fn gate(&mut self, rule_id: Uuid) -> intercept_proxy_runtime::Result<bool> {
        let Some(program) = self.programs.get(&RuleId::from_uuid(rule_id)) else {
            return Ok(true);
        };
        program
            .apply_rule_if_matches(
                ProtocolDocumentRuleId::from_uuid(rule_id),
                &mut self.document,
            )
            .map_err(|error| crate::adapters::pipeline::app_to_proxy(error.into()))
    }

    async fn encode(self: Box<Self>) -> Result<SocketContext, Error> {
        (*self).encode_socket().await
    }
}

fn external_rpc_error(
    package: ProtocolPackageRef,
    direction: ProtocolDirection,
    stage: ExternalPackageCallStage,
    default_method: &'static str,
    error: &crate::adapters::PackageTransportError,
) -> Error {
    let (method, request_id, remote_code, stable_code, remote_message, remote_data_summary) =
        match error {
            crate::adapters::PackageTransportError::Remote {
                request_id,
                method,
                error,
            } => (
                (*method).to_owned(),
                Some(request_id.clone()),
                Some(error.code()),
                Some(error.data().code().as_str().to_owned()),
                Some(error.message().to_owned()),
                Some("object(fields=1)".to_owned()),
            ),
            _ => (default_method.to_owned(), None, None, None, None, None),
        };
    Error::new(format!("EXTERNAL_PACKAGE_CALL_FAILED\n{error}")).with_external_package_call(
        ExternalPackageCallFailure {
            package,
            direction,
            stage,
            method,
            request_id,
            remote_code,
            stable_code,
            remote_message,
            remote_data_summary,
        },
    )
}
