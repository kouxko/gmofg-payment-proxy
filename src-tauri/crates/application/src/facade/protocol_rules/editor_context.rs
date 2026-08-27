use intercept_proxy_domain::{DocumentField, DocumentFieldType, DocumentSchema, DocumentSchemaId};

use super::super::{
    Application,
    protocol_packages::{ensure_description_identity, ensure_external_description},
};
use super::find_listener;
use crate::{
    AppError, AppResult, HttpBodyProcessing, ListenerDataPlane, ListenerId, ProtocolDirection,
    ProtocolPackageDescriptionViewModel, ProtocolPackageKindViewModel, ProtocolPackageRef,
    ProtocolPackageSchemaFieldTypeViewModel, ProtocolPackageSourceViewModel,
    ProtocolRuleCapabilityCatalog, ProtocolRuleCommonActionCapability, ProtocolRuleEditorContext,
    ProtocolRuleEditorStage, ProtocolRuleFieldActionCapability, ProtocolRuleFieldCapability,
    ProtocolRuleFieldOperatorCapability, ProtocolRuleSaveInput, ProtocolRuleStage, ProxyListener,
    SocketPayloadProcessing, SocketTopology,
};

impl Application {
    /// 一次返回指定入口的全部有效阶段、能力目录与 Rust 生成的新规则草稿。
    pub async fn protocol_rule_editor_context(
        &self,
        listener_id: ListenerId,
    ) -> AppResult<ProtocolRuleEditorContext> {
        let workspace = self.selected_protocol_rule_workspace().await?;
        let listener = find_listener(&workspace, listener_id)?;
        let description_context = self.protocol_rule_description_context(listener).await?;
        let default_action = if description_context.local_responder {
            intercept_proxy_domain::DocumentAction::ClearDocument
        } else {
            intercept_proxy_domain::DocumentAction::RecordMatch
        };
        let valid_stages = valid_protocol_rule_stages(description_context.local_responder);
        let mut stages = Vec::with_capacity(valid_stages.len());
        for &stage in valid_stages {
            let context = protocol_rule_context_from_description(&description_context, stage)?;
            let catalog = capability_catalog(&context, stage);
            stages.push(ProtocolRuleEditorStage {
                stage,
                schema_version: catalog.schema_version,
                fields: catalog.fields,
                common_actions: catalog.common_actions,
                new_rule_draft: ProtocolRuleSaveInput {
                    rule_id: None,
                    expected_revision: None,
                    name: "新规则".into(),
                    enabled: true,
                    priority: 100,
                    listener_id,
                    package: description_context.package.clone(),
                    schema_version: context.schema.version(),
                    stage,
                    conditions: Vec::new(),
                    actions: vec![default_action.clone()],
                },
            });
        }
        Ok(ProtocolRuleEditorContext {
            listener_id,
            package: description_context.package,
            stages,
        })
    }

    pub(super) async fn protocol_rule_context(
        &self,
        listener: &ProxyListener,
        stage: ProtocolRuleStage,
    ) -> AppResult<ProtocolRuleContext> {
        let description_context = self.protocol_rule_description_context(listener).await?;
        protocol_rule_context_from_description(&description_context, stage)
    }

    async fn protocol_rule_description_context(
        &self,
        listener: &ProxyListener,
    ) -> AppResult<ProtocolRuleDescriptionContext> {
        let package = match &listener.data_plane {
            ListenerDataPlane::Http(settings) => match &settings.body_processing {
                HttpBodyProcessing::Plain => {
                    return Err(AppError::new(
                        "DOCUMENT_RULE_PROTOCOL_REQUIRED",
                        "报文规则只能绑定已选择协议方案的入口。",
                    )
                    .entity(listener.id.to_string()));
                }
                HttpBodyProcessing::Protocol { package } => package,
            },
            ListenerDataPlane::Socket(settings) => {
                let SocketPayloadProcessing::Scripted(scripted) = &settings.processing else {
                    return Err(AppError::new(
                        "DOCUMENT_RULE_PROTOCOL_REQUIRED",
                        "报文规则只能绑定已选择协议方案的入口。",
                    )
                    .entity(listener.id.to_string()));
                };
                &scripted.package
            }
        };
        let version = self.require_protocol_package(package).await?;
        let description = match version.source {
            ProtocolPackageSourceViewModel::Internal { .. } => {
                let description = self.protocol_package_compiler.describe(package).await?;
                ensure_description_identity(package, &description)?;
                description
            }
            ProtocolPackageSourceViewModel::External { .. } => {
                let description = self.external_packages.describe(package).await?;
                ensure_external_description(package, &description)?;
                description
            }
        };
        match (&listener.data_plane, description.kind) {
            (ListenerDataPlane::Http(_), ProtocolPackageKindViewModel::Http)
            | (ListenerDataPlane::Socket(_), ProtocolPackageKindViewModel::Socket) => {}
            _ => {
                return Err(AppError::new(
                    "PROTOCOL_PACKAGE_KIND_MISMATCH",
                    "协议包类型与入口数据平面不一致。",
                )
                .entity(listener.id.to_string()));
            }
        }
        Ok(ProtocolRuleDescriptionContext {
            package: package.clone(),
            description,
            local_responder: matches!(
                &listener.data_plane,
                ListenerDataPlane::Socket(settings)
                    if matches!(settings.topology, SocketTopology::LocalResponder(_))
            ),
        })
    }
}

struct ProtocolRuleDescriptionContext {
    package: ProtocolPackageRef,
    description: ProtocolPackageDescriptionViewModel,
    local_responder: bool,
}

pub(super) struct ProtocolRuleContext {
    pub(super) package: ProtocolPackageRef,
    description: ProtocolPackageDescriptionViewModel,
    pub(super) schema: DocumentSchema,
}

fn protocol_rule_context_from_description(
    description_context: &ProtocolRuleDescriptionContext,
    stage: ProtocolRuleStage,
) -> AppResult<ProtocolRuleContext> {
    if description_context.local_responder
        && !matches!(
            stage,
            ProtocolRuleStage::AppToProxy | ProtocolRuleStage::ProxyToApp
        )
    {
        return Err(AppError::new(
            "PROTOCOL_RULE_DIRECTION_INVALID",
            "本机应答只允许配置“应用 → 代理”和“代理 → 应用”阶段。",
        ));
    }
    let schema = domain_schema(schema_for_stage(&description_context.description, stage))?;
    Ok(ProtocolRuleContext {
        package: description_context.package.clone(),
        description: description_context.description.clone(),
        schema,
    })
}

fn valid_protocol_rule_stages(local_responder: bool) -> &'static [ProtocolRuleStage] {
    const ALL: &[ProtocolRuleStage] = &[
        ProtocolRuleStage::AppToProxy,
        ProtocolRuleStage::ProxyToUpstream,
        ProtocolRuleStage::UpstreamToProxy,
        ProtocolRuleStage::ProxyToApp,
    ];
    const LOCAL_RESPONDER: &[ProtocolRuleStage] =
        &[ProtocolRuleStage::AppToProxy, ProtocolRuleStage::ProxyToApp];
    if local_responder {
        LOCAL_RESPONDER
    } else {
        ALL
    }
}

fn schema_for_stage(
    description: &ProtocolPackageDescriptionViewModel,
    stage: ProtocolRuleStage,
) -> &crate::ProtocolPackageSchemaViewModel {
    match stage.direction() {
        ProtocolDirection::Upstream => &description.upstream_schema,
        ProtocolDirection::Downstream => &description.downstream_schema,
    }
}

fn domain_schema(schema: &crate::ProtocolPackageSchemaViewModel) -> AppResult<DocumentSchema> {
    let fields = schema
        .fields
        .iter()
        .map(|field| {
            DocumentField::new(
                field.name.parse()?,
                match field.field_type {
                    ProtocolPackageSchemaFieldTypeViewModel::String => DocumentFieldType::String,
                    ProtocolPackageSchemaFieldTypeViewModel::Int => DocumentFieldType::Int,
                    ProtocolPackageSchemaFieldTypeViewModel::Bool => DocumentFieldType::Bool,
                    ProtocolPackageSchemaFieldTypeViewModel::Blob => DocumentFieldType::Blob,
                },
                field.label.clone(),
            )
        })
        .collect::<Result<Vec<_>, intercept_proxy_domain::DomainError>>()?;
    Ok(DocumentSchema::new(
        DocumentSchemaId::new(schema.id.clone())?,
        schema.version,
        schema.title.clone(),
        fields,
    )?)
}

pub(super) fn capability_catalog(
    context: &ProtocolRuleContext,
    stage: ProtocolRuleStage,
) -> ProtocolRuleCapabilityCatalog {
    let field_actions = vec![
        ProtocolRuleFieldActionCapability::SetField,
        ProtocolRuleFieldActionCapability::ClearField,
    ];
    let fields = schema_for_stage(&context.description, stage)
        .fields
        .iter()
        .map(|field| ProtocolRuleFieldCapability {
            name: field.name.clone(),
            label: field.label.clone(),
            field_type: field.field_type,
            operators: vec![ProtocolRuleFieldOperatorCapability::Equals],
            actions: field_actions.clone(),
        })
        .collect();
    let common_actions = vec![
        ProtocolRuleCommonActionCapability::RecordMatch,
        ProtocolRuleCommonActionCapability::ClearDocument,
    ];
    ProtocolRuleCapabilityCatalog {
        package: context.package.clone(),
        schema_version: context.schema.version(),
        stage,
        fields,
        common_actions,
    }
}
