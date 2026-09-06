use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use intercept_proxy_domain::{
    Document, DocumentMutation, MatchContext, ProtocolDirection, ProtocolPackageRef, RuleId,
    UnifiedAction, UnifiedRuleProgram, matches_http_condition,
};
use intercept_proxy_exchange::{
    Error, ExternalPackageCallStage, RuleProcessingAccumulator, RuleProcessingChange,
    RuleProcessingOperation, RuleProcessingOperationKind, SocketContext, rules_processed,
};
use intercept_proxy_product_api::BodyCodec;
use intercept_proxy_runtime::{
    HttpJointEvaluation, JointRuleConditionEvaluation, Message, SocketJointEvaluation,
};
use uuid::Uuid;

use crate::adapters::ProtocolPackageRuntime;

use super::package_error::external_rpc_error;

pub(crate) struct JointDocumentEvaluation {
    document: Document,
    original_document: Document,
    encoder: JointDocumentEncoder,
    programs: HashMap<RuleId, Arc<UnifiedRuleProgram>>,
    changes: RuleProcessingAccumulator,
    listener_transaction: Option<tokio::sync::OwnedMutexGuard<()>>,
}

enum JointDocumentEncoder {
    PlainJson {
        direction: ProtocolDirection,
    },
    External {
        original_input: String,
        runtime: Arc<dyn ProtocolPackageRuntime>,
        direction: ProtocolDirection,
        codec: Arc<dyn BodyCodec>,
        package: ProtocolPackageRef,
    },
    SocketExternal {
        original_input: Vec<u8>,
        runtime: Arc<dyn ProtocolPackageRuntime>,
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
    pub(crate) fn new_plain_json(
        document: Document,
        original_document: Document,
        direction: ProtocolDirection,
        programs: impl IntoIterator<Item = Arc<UnifiedRuleProgram>>,
    ) -> Self {
        Self {
            document,
            original_document,
            encoder: JointDocumentEncoder::PlainJson { direction },
            programs: program_index(programs),
            changes: RuleProcessingAccumulator::default(),
            listener_transaction: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_external(
        document: Document,
        original_document: Document,
        original_input: String,
        runtime: Arc<dyn ProtocolPackageRuntime>,
        direction: ProtocolDirection,
        codec: Arc<dyn BodyCodec>,
        package: ProtocolPackageRef,
        programs: impl IntoIterator<Item = Arc<UnifiedRuleProgram>>,
    ) -> Self {
        let programs = program_index(programs);
        Self {
            document,
            original_document,
            encoder: JointDocumentEncoder::External {
                original_input,
                runtime,
                direction,
                codec,
                package,
            },
            programs,
            changes: RuleProcessingAccumulator::default(),
            listener_transaction: None,
        }
    }

    pub(crate) fn new_external_socket(
        document: Document,
        original_document: Document,
        original_input: Vec<u8>,
        runtime: Arc<dyn ProtocolPackageRuntime>,
        direction: ProtocolDirection,
        package: ProtocolPackageRef,
        programs: impl IntoIterator<Item = Arc<UnifiedRuleProgram>>,
    ) -> Self {
        let programs = program_index(programs);
        Self {
            document,
            original_document,
            encoder: JointDocumentEncoder::SocketExternal {
                original_input,
                runtime,
                direction,
                package,
            },
            programs,
            changes: RuleProcessingAccumulator::default(),
            listener_transaction: None,
        }
    }

    pub(crate) fn with_listener_transaction(
        mut self,
        transaction: tokio::sync::OwnedMutexGuard<()>,
    ) -> Self {
        self.listener_transaction = Some(transaction);
        self
    }

    fn gate_http(
        &mut self,
        rule_id: RuleId,
        match_context: &MatchContext<'_>,
        message: &mut Message,
        apply_action: &mut dyn FnMut(
            &mut Message,
            &intercept_proxy_domain::HttpAction,
        ) -> intercept_proxy_runtime::Result<()>,
    ) -> intercept_proxy_runtime::Result<JointRuleConditionEvaluation> {
        let Some(program) = self.programs.get(&rule_id) else {
            return Ok(JointRuleConditionEvaluation::NotOwned);
        };
        let header_values = message
            .headers
            .iter()
            .map(|header| (header.name.clone(), header.value.clone()))
            .collect::<Vec<_>>();
        let headers = header_values
            .iter()
            .map(|(name, value)| intercept_proxy_domain::HttpHeader::new(name, value))
            .collect::<Vec<_>>();
        let working_context = MatchContext {
            runtime_epoch: match_context.runtime_epoch,
            channel: match_context.channel.clone(),
            stage: match_context.stage,
            terminal: match_context.terminal,
            method: match_context.method,
            request_target: match_context.request_target,
            headers: &headers,
        };
        let evaluation = program
            .evaluate_rule_with_http(rule_id, &self.document, |field, operator| {
                matches_http_condition(field, operator, &working_context)
            })
            .map_err(|error| crate::adapters::pipeline::app_to_proxy(error.into()))?;
        if evaluation.matched {
            let action = program
                .rule(rule_id)
                .expect("owned rule remains in its immutable program")
                .action();
            match action {
                UnifiedAction::Document(mutation) => mutation
                    .apply(&mut self.document)
                    .map_err(|error| crate::adapters::pipeline::app_to_proxy(error.into()))?,
                UnifiedAction::Http(
                    action @ (intercept_proxy_domain::HttpAction::SetJsonField { .. }
                    | intercept_proxy_domain::HttpAction::ReplaceBodyText(_)
                    | intercept_proxy_domain::HttpAction::SetHeader { .. }),
                ) => {
                    apply_action(message, action)?;
                }
                UnifiedAction::RecordMatch
                | UnifiedAction::Http(_)
                | UnifiedAction::Terminal(_) => {}
            }
        }
        self.changes
            .record(processing_change(program, rule_id, evaluation.matched));
        Ok(JointRuleConditionEvaluation::UnifiedOwned(
            intercept_proxy_runtime::JointConditionEvaluation {
                matched: evaluation.matched,
            },
        ))
    }

    pub(crate) async fn encode_into(self, message: &mut Message) -> Result<(), Error> {
        let direction = match &self.encoder {
            JointDocumentEncoder::PlainJson { direction }
            | JointDocumentEncoder::External { direction, .. }
            | JointDocumentEncoder::SocketExternal { direction, .. } => *direction,
        };
        rules_processed(direction, &self.changes, &self.document);
        let written = match self.encoder {
            JointDocumentEncoder::PlainJson { .. } => {
                if self.document == self.original_document {
                    return Ok(());
                }
                self.document
                    .to_json()
                    .map_err(|error| Error::coded(error.code.as_str(), error.message))?
                    .into_bytes()
            }
            JointDocumentEncoder::External {
                original_input,
                runtime,
                direction,
                codec,
                package,
            } => {
                if self.document == self.original_document {
                    return Ok(());
                }
                let encoded = runtime
                    .encode_http(direction, original_input, self.document)
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
                    .map_err(|error| Error::coded(error.code, error.message))?
            }
            JointDocumentEncoder::SocketExternal { .. } => {
                return Err(Error::coded(
                    "SOCKET_JOINT_OUTPUT_MISMATCH",
                    "Socket evaluation reached HTTP encoder",
                ));
            }
        };
        message.replace_body(written.into());
        Ok(())
    }

    async fn encode_socket(self) -> Result<SocketContext, Error> {
        let direction = match &self.encoder {
            JointDocumentEncoder::PlainJson { direction }
            | JointDocumentEncoder::External { direction, .. }
            | JointDocumentEncoder::SocketExternal { direction, .. } => *direction,
        };
        rules_processed(direction, &self.changes, &self.document);
        let JointDocumentEncoder::SocketExternal {
            original_input,
            runtime,
            direction,
            package,
        } = self.encoder
        else {
            return Err(Error::coded(
                "HTTP_JOINT_OUTPUT_MISMATCH",
                "HTTP evaluation reached Socket encoder",
            ));
        };
        if self.document == self.original_document {
            return Ok(SocketContext {
                data: original_input,
            });
        }
        let encoded = runtime
            .encode_socket(direction, original_input, self.document)
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
        Ok(SocketContext { data: encoded })
    }
}

#[async_trait]
impl HttpJointEvaluation for JointDocumentEvaluation {
    fn gate(
        &mut self,
        rule_id: Uuid,
        context: &MatchContext<'_>,
        message: &mut Message,
        apply_action: &mut dyn FnMut(
            &mut Message,
            &intercept_proxy_domain::HttpAction,
        ) -> intercept_proxy_runtime::Result<()>,
    ) -> intercept_proxy_runtime::Result<JointRuleConditionEvaluation> {
        self.gate_http(RuleId::from_uuid(rule_id), context, message, apply_action)
    }

    async fn encode(self: Box<Self>, message: &mut Message) -> Result<(), Error> {
        (*self).encode_into(message).await
    }
}

fn program_index(
    programs: impl IntoIterator<Item = Arc<UnifiedRuleProgram>>,
) -> HashMap<RuleId, Arc<UnifiedRuleProgram>> {
    programs
        .into_iter()
        .flat_map(|program| {
            program
                .rules()
                .iter()
                .map(|rule| (rule.rule_id(), Arc::clone(&program)))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[async_trait]
impl SocketJointEvaluation for JointDocumentEvaluation {
    fn gate(
        &mut self,
        rule_id: Uuid,
    ) -> intercept_proxy_runtime::Result<JointRuleConditionEvaluation> {
        let Some(program) = self.programs.get(&RuleId::from_uuid(rule_id)) else {
            return Ok(JointRuleConditionEvaluation::NotOwned);
        };
        let evaluation = program
            .evaluate_and_apply_rule_with_http(
                RuleId::from_uuid(rule_id),
                &mut self.document,
                |_, _| {
                    Err(intercept_proxy_domain::DomainError::new(
                        intercept_proxy_domain::ErrorCode::RuleInvalid,
                        "Socket rules cannot evaluate HTTP conditions",
                    ))
                },
            )
            .map_err(|error| crate::adapters::pipeline::app_to_proxy(error.into()))?;
        self.changes.record(processing_change(
            program,
            RuleId::from_uuid(rule_id),
            evaluation.matched,
        ));
        Ok(JointRuleConditionEvaluation::UnifiedOwned(
            intercept_proxy_runtime::JointConditionEvaluation {
                matched: evaluation.matched,
            },
        ))
    }

    async fn encode(self: Box<Self>) -> Result<SocketContext, Error> {
        (*self).encode_socket().await
    }
}

fn processing_change(
    program: &UnifiedRuleProgram,
    rule_id: RuleId,
    matched: bool,
) -> RuleProcessingChange {
    let operations = if matched {
        program
            .rules()
            .iter()
            .find(|rule| rule.rule_id() == rule_id)
            .map_or_else(Vec::new, |rule| {
                let operation = match rule.action() {
                    UnifiedAction::RecordMatch => Some(RuleProcessingOperation {
                        kind: RuleProcessingOperationKind::RecordMatch,
                        path: None,
                    }),
                    UnifiedAction::Document(DocumentMutation::Set { path, .. }) => {
                        Some(RuleProcessingOperation {
                            kind: RuleProcessingOperationKind::Set,
                            path: Some(path.as_str().to_owned()),
                        })
                    }
                    UnifiedAction::Document(DocumentMutation::Clear { path, .. }) => {
                        Some(RuleProcessingOperation {
                            kind: RuleProcessingOperationKind::Clear,
                            path: Some(path.as_str().to_owned()),
                        })
                    }
                    UnifiedAction::Document(DocumentMutation::Insert { path, .. }) => {
                        Some(RuleProcessingOperation {
                            kind: RuleProcessingOperationKind::Insert,
                            path: Some(path.as_str().to_owned()),
                        })
                    }
                    UnifiedAction::Document(DocumentMutation::Append { path, .. }) => {
                        Some(RuleProcessingOperation {
                            kind: RuleProcessingOperationKind::Append,
                            path: Some(path.as_str().to_owned()),
                        })
                    }
                    UnifiedAction::Http(_) | UnifiedAction::Terminal(_) => None,
                };
                operation.into_iter().collect()
            })
    } else {
        Vec::new()
    };
    RuleProcessingChange {
        rule_id: rule_id.to_string(),
        matched,
        operations,
    }
}
