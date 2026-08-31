//! Rust-authoritative editor context for the unified rule API.

mod validation;

use super::{
    Application, protocol_packages::ensure_external_description,
    rule_capabilities::stage_capability,
};
use crate::{
    AppError, AppResult, HttpBodyProcessing, HttpRuleEditorStage, ListenerDataPlane, ListenerId,
    MessageStage, ProtocolPackageDescriptionViewModel, ProtocolPackageKindViewModel,
    ProtocolPackageRef, ProtocolRuleCommonActionCapability, ProtocolRuleFieldActionCapability,
    ProtocolRuleFieldCapability, ProtocolRuleFieldOperatorCapability, ProtocolRuleStage,
    ProxyListener, RuleAction as AppRuleAction, RuleActionKind, RuleCondition as AppRuleCondition,
    RuleConditionKind, RuleContent, RuleDefinitionDraft, RuleDefinitionSaveInput,
    RuleEditorContentContext, RuleEditorContext, RuleMatchField, RuleMatchOperator, RuleStage,
    RuleTerminalAction, SocketPayloadProcessing, SocketRuleEditorStage, SocketTopology,
};
use intercept_proxy_domain::{
    Condition, ConditionTree, DocumentSchemaNode, DropResponseMode, HttpAction as DomainRuleAction,
    HttpDocumentRuleContent, HttpRuleContent, JitterScope, MatchField, MatchOperator,
    SocketRuleContent, TerminalAction as DomainTerminalAction, TrafficDirection, UnifiedAction,
};

impl Application {
    /// Returns every stage and content capability valid for the selected Listener.
    pub async fn rule_editor_context(
        &self,
        listener_id: ListenerId,
    ) -> AppResult<RuleEditorContext> {
        let workspace = self.selected_rule_workspace().await?;
        let listener = workspace
            .listeners
            .iter()
            .find(|listener| listener.id == listener_id)
            .ok_or_else(|| {
                AppError::new(
                    "RULE_LISTENER_NOT_FOUND",
                    "当前 Workspace 中不存在指定入口。",
                )
                .entity(listener_id.to_string())
            })?;
        let content = match &listener.data_plane {
            ListenerDataPlane::Http(settings) => {
                let description = match &settings.body_processing {
                    HttpBodyProcessing::Plain => None,
                    HttpBodyProcessing::Protocol { package } => {
                        Some(self.rule_package_description(listener, package).await?)
                    }
                };
                RuleEditorContentContext::Http {
                    stages: http_stages(listener_id, description.as_ref()),
                }
            }
            ListenerDataPlane::Socket(settings) => {
                let SocketPayloadProcessing::Scripted(scripted) = &settings.processing else {
                    return Err(AppError::new(
                        "DOCUMENT_RULE_PROTOCOL_REQUIRED",
                        "Socket 规则只能绑定已选择协议方案的入口。",
                    )
                    .entity(listener_id.to_string()));
                };
                let description = self
                    .rule_package_description(listener, &scripted.package)
                    .await?;
                let local_responder =
                    matches!(settings.topology, SocketTopology::LocalResponder(_));
                RuleEditorContentContext::Socket {
                    package: scripted.package.clone(),
                    stages: socket_stages(listener_id, &description, local_responder),
                }
            }
        };
        Ok(RuleEditorContext {
            listener_id,
            content,
        })
    }

    pub fn rule_definition_condition_draft(
        &self,
        kind: RuleConditionKind,
        stage: MessageStage,
    ) -> AppResult<Condition> {
        Ok(domain_condition(self.rule_condition_draft(kind, stage)))
    }

    pub fn rule_definition_action_draft(
        &self,
        kind: RuleActionKind,
        stage: MessageStage,
    ) -> AppResult<DomainRuleAction> {
        domain_action(self.rule_action_draft(kind, stage)?)
    }

    async fn rule_package_description(
        &self,
        listener: &ProxyListener,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDescriptionViewModel> {
        self.require_protocol_package(package).await?;
        let description = self.external_packages.describe(package).await?;
        ensure_external_description(package, &description)?;
        let valid_kind = matches!(
            (&listener.data_plane, description.kind),
            (
                ListenerDataPlane::Http(_),
                ProtocolPackageKindViewModel::Http
            ) | (
                ListenerDataPlane::Socket(_),
                ProtocolPackageKindViewModel::Socket
            )
        );
        if !valid_kind {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_KIND_MISMATCH",
                "协议包类型与入口数据平面不一致。",
            )
            .entity(listener.id.to_string()));
        }
        Ok(description)
    }
}

pub(super) fn domain_condition(condition: AppRuleCondition) -> Condition {
    match condition {
        AppRuleCondition::Field { field, operator } => Condition::Http {
            field: match field {
                RuleMatchField::TerminalIp => MatchField::TerminalIp,
                RuleMatchField::CertificateFingerprint => MatchField::CertificateFingerprint,
                RuleMatchField::PathOrRequestType => MatchField::PathOrRequestType,
                RuleMatchField::JsonPath { path } => MatchField::JsonPath(path),
            },
            operator: match operator {
                RuleMatchOperator::Equals { value } => MatchOperator::Equals(value),
                RuleMatchOperator::Contains { value } => MatchOperator::Contains(value),
                RuleMatchOperator::Regex { pattern } => MatchOperator::Regex(pattern),
            },
        },
        AppRuleCondition::NthHit { count } => Condition::NthHit { count },
    }
}

pub(super) fn domain_action(action: AppRuleAction) -> AppResult<DomainRuleAction> {
    Ok(match action {
        AppRuleAction::SetJsonField { path, value_json } => DomainRuleAction::SetJsonField {
            path,
            value: serde_json::from_str(&value_json).map_err(|_| {
                AppError::new("RULE_INVALID", "JSON 字段动作的默认值不是有效 JSON。")
            })?,
        },
        AppRuleAction::ReplaceBodyText { text } => DomainRuleAction::ReplaceBodyText(text),
        AppRuleAction::SetHeader { name, value } => DomainRuleAction::SetHeader { name, value },
        AppRuleAction::Delay { milliseconds } => DomainRuleAction::Delay { milliseconds },
        AppRuleAction::Jitter {
            minimum_milliseconds,
            maximum_milliseconds,
            scope,
        } => DomainRuleAction::Jitter {
            minimum_milliseconds,
            maximum_milliseconds,
            scope: match scope {
                crate::RuleJitterScope::BeforeMessage => JitterScope::BeforeMessage,
                crate::RuleJitterScope::PerChunk => JitterScope::PerChunk,
            },
        },
        AppRuleAction::Throttle {
            bytes_per_second,
            chunk_bytes,
            direction,
        } => DomainRuleAction::Throttle {
            bytes_per_second,
            chunk_bytes,
            direction: domain_direction(direction),
        },
        AppRuleAction::Intermittent {
            available_milliseconds,
            blocked_milliseconds,
            direction,
        } => DomainRuleAction::Intermittent {
            available_milliseconds,
            blocked_milliseconds,
            direction: domain_direction(direction),
        },
        AppRuleAction::Pause => DomainRuleAction::Pause,
        AppRuleAction::CustomHttpStatus { status } => DomainRuleAction::CustomHttpStatus { status },
        AppRuleAction::Terminal { action } => {
            DomainRuleAction::Terminal(domain_terminal_action(action))
        }
    })
}

fn domain_direction(direction: crate::RuleTrafficDirection) -> TrafficDirection {
    match direction {
        crate::RuleTrafficDirection::Upstream => TrafficDirection::Upstream,
        crate::RuleTrafficDirection::Downstream => TrafficDirection::Downstream,
    }
}

fn domain_terminal_action(action: RuleTerminalAction) -> DomainTerminalAction {
    match action {
        RuleTerminalAction::RejectTlsHandshake => DomainTerminalAction::RejectTlsHandshake,
        RuleTerminalAction::DisconnectBeforeUpstream => {
            DomainTerminalAction::DisconnectBeforeUpstream
        }
        RuleTerminalAction::UpstreamConnectTimeout { milliseconds } => {
            DomainTerminalAction::UpstreamConnectTimeout { milliseconds }
        }
        RuleTerminalAction::UpstreamWriteTimeout { milliseconds } => {
            DomainTerminalAction::UpstreamWriteTimeout { milliseconds }
        }
        RuleTerminalAction::UpstreamReadTimeout { milliseconds } => {
            DomainTerminalAction::UpstreamReadTimeout { milliseconds }
        }
        RuleTerminalAction::DropUpstreamResponse { mode } => {
            DomainTerminalAction::DropUpstreamResponse {
                mode: match mode {
                    crate::RuleDropResponseMode::ReadCompleteResponse => {
                        DropResponseMode::ReadCompleteResponse
                    }
                    crate::RuleDropResponseMode::CloseAfterRequestWrite => {
                        DropResponseMode::CloseAfterRequestWrite
                    }
                },
            }
        }
        RuleTerminalAction::MockResponse {
            status,
            headers,
            body_bytes,
        } => DomainTerminalAction::MockResponse {
            status,
            headers,
            body_bytes,
        },
        RuleTerminalAction::InvalidJson { body_bytes } => {
            DomainTerminalAction::InvalidJson { body_bytes }
        }
        RuleTerminalAction::IncorrectContentLength { delta } => {
            DomainTerminalAction::IncorrectContentLength { delta }
        }
        RuleTerminalAction::TruncateResponse { bytes } => {
            DomainTerminalAction::TruncateResponse { bytes }
        }
        RuleTerminalAction::DisconnectDuringUpstreamWrite { after_bytes } => {
            DomainTerminalAction::DisconnectDuringUpstreamWrite { after_bytes }
        }
        RuleTerminalAction::DisconnectDuringDownstreamWrite { after_bytes } => {
            DomainTerminalAction::DisconnectDuringDownstreamWrite { after_bytes }
        }
    }
}

fn http_stages(
    listener_id: ListenerId,
    description: Option<&ProtocolPackageDescriptionViewModel>,
) -> Vec<HttpRuleEditorStage> {
    [
        RuleStage::TlsHandshake,
        RuleStage::ProxyToUpstream,
        RuleStage::ProxyToApp,
    ]
    .into_iter()
    .filter_map(|stage| {
        let http = Some(http_capability(stage));
        let document = description.and_then(|value| document_capability(value, stage));
        if http.is_none() && document.is_none() {
            return None;
        }
        let package = document.as_ref().map(|_| {
            description
                .expect("document requires description")
                .package
                .clone()
        });
        let document_fields = document
            .as_ref()
            .map(|value| value.fields.clone())
            .unwrap_or_default();
        let document_common_actions = document
            .as_ref()
            .map(|value| value.common_actions.clone())
            .unwrap_or_default();
        let embedded_document = document.as_ref().map(|_| HttpDocumentRuleContent {
            package: package.clone().expect("document stage has a package"),
        });
        Some(HttpRuleEditorStage {
            stage,
            http,
            package,
            document_fields,
            document_common_actions,
            new_rule_draft: RuleDefinitionSaveInput {
                rule_id: None,
                expected_revision: None,
                draft: RuleDefinitionDraft {
                    name: "新规则".into(),
                    enabled: true,
                    priority: 100,
                    listener_id,
                    stage,
                    one_shot: false,
                    content: RuleContent::Http(HttpRuleContent {
                        description: String::new(),
                        condition: ConditionTree::All(Vec::new()),
                        actions: vec![UnifiedAction::RecordMatch],
                        document: embedded_document,
                    }),
                },
            },
        })
    })
    .collect()
}

fn socket_stages(
    listener_id: ListenerId,
    description: &ProtocolPackageDescriptionViewModel,
    local_responder: bool,
) -> Vec<SocketRuleEditorStage> {
    let _ = local_responder;
    let stages: &[RuleStage] = &[RuleStage::ProxyToUpstream, RuleStage::ProxyToApp];
    stages
        .iter()
        .filter_map(|&stage| {
            document_capability(description, stage).map(|catalog| (stage, catalog))
        })
        .map(|(stage, catalog)| SocketRuleEditorStage {
            stage,
            fields: catalog.fields.clone(),
            common_actions: catalog.common_actions.clone(),
            new_rule_draft: RuleDefinitionSaveInput {
                rule_id: None,
                expected_revision: None,
                draft: RuleDefinitionDraft {
                    name: "新规则".into(),
                    enabled: true,
                    priority: 100,
                    listener_id,
                    stage,
                    one_shot: false,
                    content: RuleContent::Socket(SocketRuleContent {
                        package: description.package.clone(),
                        condition: ConditionTree::All(Vec::new()),
                        actions: vec![UnifiedAction::RecordMatch],
                    }),
                },
            },
        })
        .collect()
}

fn http_capability(stage: RuleStage) -> crate::RuleStageCapabilityViewModel {
    match stage {
        RuleStage::TlsHandshake => stage_capability(MessageStage::TlsHandshake),
        RuleStage::ProxyToUpstream => stage_capability(MessageStage::Request),
        RuleStage::ProxyToApp => stage_capability(MessageStage::Response),
    }
}

#[derive(Clone)]
struct DocumentCapability {
    fields: Vec<ProtocolRuleFieldCapability>,
    common_actions: Vec<ProtocolRuleCommonActionCapability>,
}

fn document_capability(
    description: &ProtocolPackageDescriptionViewModel,
    stage: RuleStage,
) -> Option<DocumentCapability> {
    let protocol_stage = match stage {
        RuleStage::ProxyToUpstream => ProtocolRuleStage::ProxyToUpstream,
        RuleStage::ProxyToApp => ProtocolRuleStage::ProxyToApp,
        RuleStage::TlsHandshake => {
            return None;
        }
    };
    let schema = match protocol_stage.direction() {
        intercept_proxy_domain::ProtocolDirection::Upstream => &description.upstream_schema,
        intercept_proxy_domain::ProtocolDirection::Downstream => &description.downstream_schema,
    };
    let actions = vec![
        ProtocolRuleFieldActionCapability::SetField,
        ProtocolRuleFieldActionCapability::ClearField,
    ];
    let mut fields = Vec::new();
    if let Some(schema) = schema {
        collect_document_schema_fields(&schema.root, "", &actions, &mut fields);
    }
    Some(DocumentCapability {
        fields,
        common_actions: vec![ProtocolRuleCommonActionCapability::RecordMatch],
    })
}

fn collect_document_schema_fields(
    schema: &DocumentSchemaNode,
    path: &str,
    actions: &[ProtocolRuleFieldActionCapability],
    output: &mut Vec<ProtocolRuleFieldCapability>,
) {
    let field_type = match schema {
        DocumentSchemaNode::String { .. } => crate::ProtocolPackageSchemaFieldTypeViewModel::String,
        DocumentSchemaNode::Number { .. } => crate::ProtocolPackageSchemaFieldTypeViewModel::Number,
        DocumentSchemaNode::Boolean { .. } => {
            crate::ProtocolPackageSchemaFieldTypeViewModel::Boolean
        }
        DocumentSchemaNode::Object { properties, .. } => {
            for (name, child) in properties {
                collect_document_schema_fields(
                    child,
                    &format!("{path}/{}", name.replace('~', "~0").replace('/', "~1")),
                    actions,
                    output,
                );
            }
            crate::ProtocolPackageSchemaFieldTypeViewModel::Object
        }
        DocumentSchemaNode::Array { items, .. } => {
            collect_document_schema_fields(items, &format!("{path}/0"), actions, output);
            crate::ProtocolPackageSchemaFieldTypeViewModel::Array
        }
    };
    if !path.is_empty() {
        output.push(ProtocolRuleFieldCapability {
            name: path.to_owned(),
            label: schema.title().unwrap_or(path).to_owned(),
            field_type,
            operators: vec![ProtocolRuleFieldOperatorCapability::Equals],
            actions: actions.to_vec(),
        });
    }
}
