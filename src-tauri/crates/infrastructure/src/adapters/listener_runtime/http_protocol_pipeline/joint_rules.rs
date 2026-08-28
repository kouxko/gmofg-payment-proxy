use std::{collections::HashMap, sync::Arc};

use intercept_proxy_domain::{
    Document, ProtocolDocumentRuleId, ProtocolDocumentRuleProgram, Rule, RuleId,
};
use intercept_proxy_runtime::{ConnectionContext, Message};
use parking_lot::Mutex;
use uuid::Uuid;

use super::{SharedExecutor, run_stage};

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
    origin: Vec<u8>,
    executor: SharedExecutor,
    programs: HashMap<RuleId, Arc<ProtocolDocumentRuleProgram>>,
}

impl std::fmt::Debug for JointDocumentEvaluation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JointDocumentEvaluation")
            .field("schema", self.document.schema())
            .field("rule_count", &self.programs.len())
            .finish_non_exhaustive()
    }
}

impl JointDocumentEvaluation {
    pub(super) fn new(
        document: Document,
        origin: Vec<u8>,
        executor: SharedExecutor,
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
            origin,
            executor,
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

    pub(crate) async fn encode_into(self, message: &mut Message) -> Result<(), String> {
        let executor = Arc::clone(&self.executor);
        let document = self.document;
        let written = run_stage(move || executor.lock().encode_document(&self.origin, document))
            .await
            .map_err(|error| error.to_string())?;
        std::str::from_utf8(&written).map_err(|_| {
            "HTTP_PROTOCOL_OUTPUT_NOT_UTF8: 协议包 Encode 返回了非 UTF-8 HTTP Body".to_owned()
        })?;
        message.replace_body(written.into());
        Ok(())
    }
}
