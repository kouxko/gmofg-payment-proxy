//! 持久化前基于 Listener 与协议包描述的统一 Document 规则校验。

use intercept_proxy_domain::{
    Condition, DocumentSchemaNode, RuleContent, UnifiedAction, contains_document_condition,
    validate_document_conditions_schema, validate_unified_actions_schema,
};

use super::Application;
use crate::{
    AppError, AppResult, HttpBodyProcessing, ListenerDataPlane, ProtocolPackageSchemaViewModel,
    ProxyListener, ProxyWorkspace, RuleDefinition, RuleStage, SocketPayloadProcessing,
    SocketTopology,
};

impl Application {
    pub(in crate::facade) async fn validate_rule_definition_document(
        &self,
        workspace: &ProxyWorkspace,
        rule: &RuleDefinition,
    ) -> AppResult<()> {
        let listener = workspace
            .listeners
            .iter()
            .find(|listener| listener.id == rule.listener_id())
            .ok_or_else(|| {
                rule_invalid("listener_id", "当前 Workspace 中不存在规则绑定的 Listener")
            })?;
        let Some(candidate) = document_candidate(listener, rule)? else {
            return Ok(());
        };
        let Some(package) = candidate.package else {
            return Ok(());
        };
        let description = self.rule_package_description(listener, package).await?;
        let (schema, capabilities) = match rule.stage().direction() {
            intercept_proxy_domain::ProtocolDirection::Upstream => (
                description.upstream_schema.as_ref().map(domain_schema),
                description.capabilities.upstream,
            ),
            intercept_proxy_domain::ProtocolDirection::Downstream => (
                description.downstream_schema.as_ref().map(domain_schema),
                description.capabilities.downstream,
            ),
        };
        if !capabilities.decode {
            return Err(rule_invalid(
                "stage",
                "协议包未声明该方向的 Document Decode 能力",
            ));
        }
        if candidate.actions.iter().any(document_action_modifies_value) && !capabilities.encode {
            return Err(rule_invalid(
                "content.actions",
                "协议包未声明该方向的 Document Encode 能力",
            ));
        }
        if let Some(schema) = schema {
            validate_document_conditions_schema(candidate.conditions, &schema)?;
            validate_unified_actions_schema(candidate.actions, &schema)?;
        }
        Ok(())
    }
}

struct DocumentCandidate<'a> {
    package: Option<&'a intercept_proxy_domain::ProtocolPackageRef>,
    conditions: &'a [Condition],
    actions: &'a [UnifiedAction],
}

fn document_candidate<'a>(
    listener: &'a ProxyListener,
    rule: &'a RuleDefinition,
) -> AppResult<Option<DocumentCandidate<'a>>> {
    match (rule.content(), &listener.data_plane) {
        (RuleContent::Http(content), ListenerDataPlane::Http(settings)) => {
            if !contains_document_condition(&content.conditions)
                && !content.actions.iter().any(document_action_modifies_value)
            {
                return Ok(None);
            }
            Ok(Some(DocumentCandidate {
                package: match &settings.body_processing {
                    HttpBodyProcessing::Plain => None,
                    HttpBodyProcessing::Protocol { package } => Some(package),
                },
                conditions: &content.conditions,
                actions: &content.actions,
            }))
        }
        (RuleContent::Socket(content), ListenerDataPlane::Socket(settings)) => {
            let SocketPayloadProcessing::Scripted(scripted) = &settings.processing else {
                return Err(rule_invalid(
                    "content.package",
                    "Socket Document 规则要求 Listener 使用协议包处理消息",
                ));
            };
            if content.package != scripted.package {
                return Err(rule_invalid(
                    "content.package",
                    "规则协议包必须与 Listener 的精确绑定一致",
                ));
            }
            if matches!(settings.topology, SocketTopology::LocalResponder(_))
                && !matches!(
                    rule.stage(),
                    RuleStage::ProxyToUpstream | RuleStage::ProxyToApp
                )
            {
                return Err(rule_invalid("stage", "本机应答只支持统一的代理写出阶段"));
            }
            Ok(Some(DocumentCandidate {
                package: Some(&scripted.package),
                conditions: &content.conditions,
                actions: &content.actions,
            }))
        }
        (RuleContent::Http(_), ListenerDataPlane::Socket(_))
        | (RuleContent::Socket(_), ListenerDataPlane::Http(_)) => Err(rule_invalid(
            "content",
            "规则内容类型必须与 Listener 数据平面一致",
        )),
    }
}

fn document_action_modifies_value(action: &UnifiedAction) -> bool {
    matches!(action, UnifiedAction::Document(_))
}

fn rule_invalid(field: &str, message: &str) -> AppError {
    AppError::field(
        "RULE_INVALID",
        "统一规则 Document 配置无效。",
        [(field.to_owned(), vec![message.to_owned()])]
            .into_iter()
            .collect(),
    )
}

fn domain_schema(schema: &ProtocolPackageSchemaViewModel) -> DocumentSchemaNode {
    schema.root.clone()
}
