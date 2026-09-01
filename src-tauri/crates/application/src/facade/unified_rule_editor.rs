//! Rust-authoritative editor context for the unified rule API.

mod document_factory;
mod validation;

use super::{
    Application,
    protocol_packages::ensure_external_description,
    rule_capabilities::{action_capability, stage_capability},
};
use crate::{
    AppError, AppResult, HttpBodyProcessing, HttpRuleEditorStageViewModel, ListenerDataPlane,
    ListenerId, ProtocolPackageDescriptionViewModel, ProtocolPackageKindViewModel,
    ProtocolPackageRef, ProxyListener, RuleAction as AppRuleAction, RuleCommonActionCapability,
    RuleContent, RuleDocumentConditionPathCapability, RuleEditorContentContext, RuleEditorContext,
    RuleHttpActionDraftInput, RuleLocalDocumentActionKind, RuleLocalDocumentPredicateKind,
    RuleLocalDocumentValueType, RuleMatchFieldKind, RuleMatchOperatorKind, RuleNewDefinitionDraft,
    RuleNthHitConditionDraftInput, RuleStage, RuleTerminalAction, SocketPayloadProcessing,
    SocketRuleEditorStageViewModel, SocketTopology, document_schema_field_capabilities,
    local_document_type_capabilities,
};
use intercept_proxy_domain::{
    Condition, DocumentSchemaNode, DropResponseMode, HttpAction as DomainRuleAction,
    HttpRuleContent, JitterScope, MatchField, MatchOperator, SocketRuleContent,
    TerminalAction as DomainTerminalAction, TrafficDirection, UnifiedAction,
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
            local_document_types: local_document_type_capabilities(),
            document_condition_path: RuleDocumentConditionPathCapability {
                wildcard_token: "*".into(),
                wildcard_matches_exactly_one_level: true,
                multiple_matches_use_any: true,
            },
        })
    }

    pub fn rule_definition_document_condition_draft(
        &self,
        path: &str,
        value_type: RuleLocalDocumentValueType,
        predicate: RuleLocalDocumentPredicateKind,
        raw: &str,
    ) -> AppResult<Condition> {
        document_factory::condition_draft(path, value_type, predicate, raw)
    }

    pub fn rule_definition_document_action_draft(
        &self,
        path: &str,
        value_type: RuleLocalDocumentValueType,
        action: RuleLocalDocumentActionKind,
        raw: Option<&str>,
        index: Option<u32>,
    ) -> AppResult<UnifiedAction> {
        document_factory::action_draft(path, value_type, action, raw, index)
    }

    pub fn rule_definition_document_common_action_draft(
        &self,
        action: RuleCommonActionCapability,
    ) -> UnifiedAction {
        match action {
            RuleCommonActionCapability::RecordMatch => UnifiedAction::RecordMatch,
        }
    }

    pub fn rule_definition_nth_hit_condition_draft(
        &self,
        input: RuleNthHitConditionDraftInput,
    ) -> AppResult<Condition> {
        let _ = self;
        if input.count == 0 {
            return Err(AppError::new("RULE_INVALID", "第 N 次命中必须是正整数。"));
        }
        Ok(Condition::NthHit { count: input.count })
    }

    pub fn rule_definition_http_condition_draft(
        &self,
        field_kind: RuleMatchFieldKind,
        selector: Option<&str>,
        operator_kind: RuleMatchOperatorKind,
        value: &str,
        stage: RuleStage,
    ) -> AppResult<Condition> {
        let capability = stage_capability(stage)
            .match_fields
            .into_iter()
            .find(|capability| capability.kind == field_kind)
            .ok_or_else(|| AppError::new("RULE_INVALID", "匹配字段与当前规则阶段不兼容。"))?;
        if !capability.operators.contains(&operator_kind) {
            return Err(AppError::new(
                "RULE_INVALID",
                "匹配操作符与所选字段不兼容。",
            ));
        }
        let field = match field_kind {
            RuleMatchFieldKind::TerminalIp => MatchField::TerminalIp,
            RuleMatchFieldKind::CertificateFingerprint => MatchField::CertificateFingerprint,
            RuleMatchFieldKind::Method => MatchField::Method,
            RuleMatchFieldKind::RequestTarget => MatchField::RequestTarget,
            RuleMatchFieldKind::Header => MatchField::Header(
                selector
                    .ok_or_else(|| AppError::new("RULE_INVALID", "Header 条件需要 /name。"))?
                    .to_owned(),
            ),
        };
        let operator = match operator_kind {
            RuleMatchOperatorKind::Equals => MatchOperator::Equals(value.to_owned()),
            RuleMatchOperatorKind::Contains => MatchOperator::Contains(value.to_owned()),
            RuleMatchOperatorKind::StartsWith => MatchOperator::StartsWith(value.to_owned()),
            RuleMatchOperatorKind::EndsWith => MatchOperator::EndsWith(value.to_owned()),
            RuleMatchOperatorKind::Wildcard => MatchOperator::Wildcard(value.to_owned()),
        };
        intercept_proxy_domain::validate_http_condition(&field, &operator)?;
        Ok(Condition::Http { field, operator })
    }

    pub fn rule_definition_action_draft(
        &self,
        input: RuleHttpActionDraftInput,
        stage: RuleStage,
    ) -> AppResult<DomainRuleAction> {
        let capability = action_capability(stage, input.kind)
            .ok_or_else(|| AppError::new("RULE_INVALID", "动作与当前规则阶段不兼容。"))?;
        let action = parse_app_action(input, capability.terminal, capability.parameters_required)?;
        domain_action(action)
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

fn parse_app_action(
    input: RuleHttpActionDraftInput,
    terminal: bool,
    parameters_required: bool,
) -> AppResult<AppRuleAction> {
    let mut value = if parameters_required {
        let parameters = input
            .parameters_json
            .ok_or_else(|| AppError::new("RULE_INVALID", "当前动作需要显式参数。"))?;
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&parameters)
            .map_err(|_| AppError::new("RULE_INVALID", "动作参数必须是 JSON 对象。"))?
    } else {
        if input.parameters_json.is_some() {
            return Err(AppError::new("RULE_INVALID", "无参数动作不能提交参数。"));
        }
        serde_json::Map::new()
    };
    let kind = serde_json::to_value(input.kind)
        .map_err(|_| AppError::new("RULE_INVALID", "动作类型无法序列化。"))?;
    value.insert("type".into(), kind);
    let value = if terminal {
        serde_json::json!({ "type": "terminal", "action": value })
    } else {
        serde_json::Value::Object(value)
    };
    serde_json::from_value(value)
        .map_err(|_| AppError::new("RULE_INVALID", "动作参数与动作类型不匹配。"))
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
) -> Vec<HttpRuleEditorStageViewModel> {
    [RuleStage::ProxyToUpstream, RuleStage::ProxyToApp]
        .into_iter()
        .map(|stage| {
            let http = Some(http_capability(stage));
            let document = description.map(|value| document_capability(value, stage));
            let package = description.map(|value| value.package.clone());
            let document_fields = document
                .as_ref()
                .and_then(|value| value.schema.as_ref())
                .map(document_schema_field_capabilities)
                .unwrap_or_default();
            let document_common_actions = document.map_or_else(
                || vec![RuleCommonActionCapability::RecordMatch],
                |value| value.common_actions.clone(),
            );
            HttpRuleEditorStageViewModel {
                stage,
                http,
                package,
                document_fields,
                document_common_actions,
                new_rule_draft: RuleNewDefinitionDraft {
                    listener_id,
                    stage,
                    content: RuleContent::Http(HttpRuleContent {
                        description: String::new(),
                        conditions: Vec::new(),
                        actions: Vec::new(),
                    }),
                },
            }
        })
        .collect()
}

fn socket_stages(
    listener_id: ListenerId,
    description: &ProtocolPackageDescriptionViewModel,
    local_responder: bool,
) -> Vec<SocketRuleEditorStageViewModel> {
    let _ = local_responder;
    let stages: &[RuleStage] = &[RuleStage::ProxyToUpstream, RuleStage::ProxyToApp];
    stages
        .iter()
        .map(|&stage| (stage, document_capability(description, stage)))
        .map(|(stage, catalog)| SocketRuleEditorStageViewModel {
            stage,
            document_fields: catalog
                .schema
                .as_ref()
                .map(document_schema_field_capabilities)
                .unwrap_or_default(),
            common_actions: catalog.common_actions.clone(),
            new_rule_draft: RuleNewDefinitionDraft {
                listener_id,
                stage,
                content: RuleContent::Socket(SocketRuleContent {
                    package: description.package.clone(),
                    conditions: Vec::new(),
                    actions: Vec::new(),
                }),
            },
        })
        .collect()
}

fn http_capability(stage: RuleStage) -> crate::RuleStageCapabilityViewModel {
    stage_capability(stage)
}

#[derive(Clone)]
struct DocumentCapability {
    schema: Option<DocumentSchemaNode>,
    common_actions: Vec<RuleCommonActionCapability>,
}

fn document_capability(
    description: &ProtocolPackageDescriptionViewModel,
    stage: RuleStage,
) -> DocumentCapability {
    let schema = match stage {
        RuleStage::ProxyToUpstream => &description.upstream_schema,
        RuleStage::ProxyToApp => &description.downstream_schema,
    };
    DocumentCapability {
        schema: schema.as_ref().map(|schema| schema.root.clone()),
        common_actions: vec![RuleCommonActionCapability::RecordMatch],
    }
}
