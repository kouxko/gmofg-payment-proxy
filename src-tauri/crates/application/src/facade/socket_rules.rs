//! 当前 Workspace 的 Socket Document 规则用例。
//!
//! 查询和写入都从 Listener 的精确协议包绑定重新取得编译描述；任何包、Schema、方向或
//! 编译缓存身份不一致都 fail-closed，前端能力目录不能替代保存门禁。

use intercept_proxy_domain::{
    DocumentField, DocumentFieldType, DocumentSchema, DocumentSchemaId,
    MAX_JAVASCRIPT_SAFE_INTEGER, Revision, sort_socket_document_rules,
};

use super::{
    Application, protocol_packages::ensure_description_identity, validation::require_confirmation,
};
use crate::{
    AppError, AppResult, DirectionProcessingOptions, ListenerDataPlane, ListenerId,
    OperationResultViewModel, ProtocolPackageCapabilitiesViewModel,
    ProtocolPackageDescriptionViewModel, ProtocolPackageRef,
    ProtocolPackageSchemaFieldTypeViewModel, ProxyListener, ProxyWorkspace, SocketDirection,
    SocketDocumentRuleDefinition, SocketDocumentRuleDraft, SocketDocumentRuleId,
    SocketPayloadProcessing, SocketRuleCapabilityCatalog, SocketRuleCommonActionCapability,
    SocketRuleFieldActionCapability, SocketRuleFieldCapability, SocketRuleFieldOperatorCapability,
    SocketRuleSaveInput, SocketTopology, UiTone, WorkspaceChangeKind,
};

impl Application {
    /// 返回当前选中 Workspace 的规则，并按运行时确定顺序稳定排序。
    pub async fn socket_rule_list(&self) -> AppResult<Vec<SocketDocumentRuleDefinition>> {
        let mut rules = self.selected_socket_rule_workspace().await?.socket_rules;
        sort_socket_document_rules(&mut rules);
        Ok(rules)
    }

    /// 从精确 Listener 绑定、编译描述与方向开关生成无 HTTP 字段的能力目录。
    pub async fn socket_rule_capabilities(
        &self,
        listener_id: ListenerId,
        direction: SocketDirection,
    ) -> AppResult<SocketRuleCapabilityCatalog> {
        let workspace = self.selected_socket_rule_workspace().await?;
        let listener = find_listener(&workspace, listener_id)?;
        let context = self.socket_rule_context(listener, direction).await?;
        Ok(capability_catalog(context, direction))
    }

    /// 新建或乐观锁更新规则。更新不能改变 Listener/包/Schema/方向绑定。
    pub async fn socket_rule_save(
        &self,
        input: SocketRuleSaveInput,
    ) -> AppResult<SocketDocumentRuleDefinition> {
        let _gate = self.mutation_gate.lock().await;
        let mut workspace = self.selected_socket_rule_workspace().await?;

        // 更新先验证本地实体、revision 与冻结绑定，不能让伪造绑定提前触发另一个包的编译查询。
        match (input.rule_id, input.expected_revision) {
            (Some(rule_id), Some(expected_revision)) => {
                let current = workspace
                    .socket_rules
                    .iter()
                    .find(|rule| rule.rule_id() == rule_id)
                    .ok_or_else(|| socket_rule_not_found(rule_id))?;
                current
                    .revision()
                    .verify(Revision::new(expected_revision))?;
                ensure_immutable_binding(current, &input)?;
            }
            (None, None) => {}
            _ => {
                return Err(AppError::new(
                    "SOCKET_RULE_REVISION_REQUIRED",
                    "创建规则不能提供 revision；更新规则必须同时提供规则 ID 与期望 revision。",
                ));
            }
        }

        self.ensure_workspace_not_running(&workspace).await?;
        let listener = find_listener(&workspace, input.listener_id)?.clone();
        let context = self.socket_rule_context(&listener, input.direction).await?;
        ensure_requested_binding(&input, &context.package, context.schema.version())?;

        let saved_rule_id = match (input.rule_id, input.expected_revision) {
            (None, None) => {
                let created_order = next_created_order(
                    &workspace.socket_rules,
                    workspace.socket_rule_created_order_high_water,
                )?;
                let rule_id = SocketDocumentRuleId::new();
                let rule = SocketDocumentRuleDefinition::new(
                    rule_id,
                    input.enabled,
                    input.priority,
                    created_order,
                    input.listener_id,
                    input.package,
                    input.schema_version,
                    input.direction,
                    input.conditions,
                    input.actions,
                )?;
                validate_rule(&rule, &context)?;
                workspace.socket_rule_created_order_high_water = created_order;
                workspace.socket_rules.push(rule);
                rule_id
            }
            (Some(rule_id), Some(expected_revision)) => {
                let current = workspace
                    .socket_rules
                    .iter_mut()
                    .find(|rule| rule.rule_id() == rule_id)
                    .ok_or_else(|| socket_rule_not_found(rule_id))?;
                ensure_immutable_binding(current, &input)?;
                let values = SocketDocumentRuleDraft {
                    enabled: input.enabled,
                    priority: input.priority,
                    listener_id: input.listener_id,
                    package: input.package,
                    schema_version: input.schema_version,
                    direction: input.direction,
                    conditions: input.conditions,
                    actions: input.actions,
                };
                current.update(Revision::new(expected_revision), values)?;
                validate_rule(current, &context)?;
                rule_id
            }
            _ => unreachable!("rule ID/revision pair was validated before external queries"),
        };

        sort_socket_document_rules(&mut workspace.socket_rules);
        workspace.validate()?;
        let saved = self.workspaces.save(workspace).await?;
        self.publish_workspace(&saved, true, WorkspaceChangeKind::Updated);
        saved
            .socket_rules
            .into_iter()
            .find(|rule| rule.rule_id() == saved_rule_id)
            .ok_or_else(|| socket_rule_not_found(saved_rule_id))
    }

    pub async fn socket_rule_toggle(
        &self,
        rule_id: SocketDocumentRuleId,
        expected_revision: u64,
        enabled: bool,
    ) -> AppResult<SocketDocumentRuleDefinition> {
        let _gate = self.mutation_gate.lock().await;
        let mut workspace = self.selected_socket_rule_workspace().await?;
        self.ensure_workspace_not_running(&workspace).await?;
        let current = workspace
            .socket_rules
            .iter_mut()
            .find(|rule| rule.rule_id() == rule_id)
            .ok_or_else(|| socket_rule_not_found(rule_id))?;
        current.set_enabled(Revision::new(expected_revision), enabled)?;
        let result = current.clone();
        sort_socket_document_rules(&mut workspace.socket_rules);
        let saved = self.workspaces.save(workspace).await?;
        self.publish_workspace(&saved, true, WorkspaceChangeKind::Updated);
        Ok(result)
    }

    pub async fn socket_rule_delete(
        &self,
        rule_id: SocketDocumentRuleId,
        expected_revision: u64,
        confirmed: bool,
    ) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        require_confirmation(confirmed, "删除 Socket 规则需要确认。")?;
        let mut workspace = self.selected_socket_rule_workspace().await?;
        self.ensure_workspace_not_running(&workspace).await?;
        let index = workspace
            .socket_rules
            .iter()
            .position(|rule| rule.rule_id() == rule_id)
            .ok_or_else(|| socket_rule_not_found(rule_id))?;
        workspace.socket_rules[index]
            .revision()
            .verify(Revision::new(expected_revision))?;
        workspace.socket_rule_created_order_high_water = workspace
            .socket_rule_created_order_high_water
            .max(workspace.socket_rules[index].created_order());
        workspace.socket_rules.remove(index);
        let saved = self.workspaces.save(workspace).await?;
        self.publish_workspace(&saved, true, WorkspaceChangeKind::Updated);
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "Socket 规则已删除。".into(),
            ui_tone: UiTone::Positive,
            entity_id: Some(rule_id.to_string()),
            revision: Some(saved.revision.get()),
            requires_restart: false,
        })
    }

    async fn selected_socket_rule_workspace(&self) -> AppResult<ProxyWorkspace> {
        let selected = self
            .workspaces
            .list()
            .await?
            .into_iter()
            .find(|summary| summary.selected)
            .ok_or_else(|| AppError::new("WORKSPACE_NOT_SELECTED", "请先选择一个 Workspace。"))?;
        self.workspaces.get(selected.id).await
    }

    async fn socket_rule_context(
        &self,
        listener: &ProxyListener,
        direction: SocketDirection,
    ) -> AppResult<SocketRuleContext> {
        let ListenerDataPlane::Socket(settings) = &listener.data_plane else {
            return Err(AppError::new(
                "SOCKET_RULE_LISTENER_REQUIRED",
                "Socket 规则只能绑定 Socket Listener。",
            )
            .entity(listener.id.to_string()));
        };
        let SocketPayloadProcessing::Scripted(scripted) = &settings.processing else {
            return Err(AppError::new(
                "SOCKET_RULE_SCRIPTED_LISTENER_REQUIRED",
                "Socket 规则只能绑定已选择协议包的 Scripted Listener。",
            )
            .entity(listener.id.to_string()));
        };
        self.require_protocol_package(&scripted.package).await?;
        let description = self
            .protocol_package_compiler
            .describe(&scripted.package)
            .await?;
        ensure_description_identity(&scripted.package, &description)?;
        let can_modify = validate_direction(
            &settings.topology,
            scripted.upstream,
            scripted.downstream,
            description.capabilities,
            direction,
        )?;
        let schema = domain_schema(&description)?;
        Ok(SocketRuleContext {
            package: scripted.package.clone(),
            description,
            schema,
            can_modify,
        })
    }
}

struct SocketRuleContext {
    package: ProtocolPackageRef,
    description: ProtocolPackageDescriptionViewModel,
    schema: DocumentSchema,
    can_modify: bool,
}

fn find_listener(workspace: &ProxyWorkspace, listener_id: ListenerId) -> AppResult<&ProxyListener> {
    workspace
        .listeners
        .iter()
        .find(|listener| listener.id == listener_id)
        .ok_or_else(|| {
            AppError::new(
                "SOCKET_RULE_LISTENER_NOT_FOUND",
                "当前 Workspace 中不存在指定的 Socket Listener。",
            )
            .entity(listener_id.to_string())
        })
}

fn validate_direction(
    topology: &SocketTopology,
    upstream: DirectionProcessingOptions,
    downstream: DirectionProcessingOptions,
    capabilities: ProtocolPackageCapabilitiesViewModel,
    direction: SocketDirection,
) -> AppResult<bool> {
    let (options, manifest) = match direction {
        SocketDirection::Upstream => (upstream, capabilities.upstream),
        SocketDirection::Downstream => (downstream, capabilities.downstream),
    };
    match topology {
        SocketTopology::Relay(_) => {
            if !options.decode_enabled || !manifest.decode {
                return Err(AppError::new(
                    "SOCKET_RULE_DECODE_REQUIRED",
                    "Relay 规则要求所选方向同时开启并声明 Decode。",
                ));
            }
        }
        SocketTopology::LocalResponder(_) if direction != SocketDirection::Downstream => {
            return Err(AppError::new(
                "SOCKET_RULE_DIRECTION_INVALID",
                "LocalResponder 只允许 downstream 响应规则。",
            ));
        }
        SocketTopology::LocalResponder(_) => {}
    }
    Ok(options.encode_enabled && manifest.encode)
}

fn domain_schema(description: &ProtocolPackageDescriptionViewModel) -> AppResult<DocumentSchema> {
    let fields = description
        .schema
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
        DocumentSchemaId::new(description.schema.id.clone())?,
        description.schema.version,
        description.schema.title.clone(),
        fields,
    )?)
}

fn capability_catalog(
    context: SocketRuleContext,
    direction: SocketDirection,
) -> SocketRuleCapabilityCatalog {
    let field_actions = context
        .can_modify
        .then_some(SocketRuleFieldActionCapability::SetField)
        .into_iter()
        .collect::<Vec<_>>();
    let fields = context
        .description
        .schema
        .fields
        .into_iter()
        .map(|field| SocketRuleFieldCapability {
            name: field.name,
            label: field.label,
            field_type: field.field_type,
            operators: vec![SocketRuleFieldOperatorCapability::Equals],
            actions: field_actions.clone(),
        })
        .collect();
    let mut common_actions = vec![SocketRuleCommonActionCapability::RecordMatch];
    if context.can_modify {
        common_actions.push(SocketRuleCommonActionCapability::ClearDocument);
    }
    SocketRuleCapabilityCatalog {
        package: context.package,
        schema_version: context.schema.version(),
        direction,
        fields,
        common_actions,
    }
}

fn ensure_requested_binding(
    input: &SocketRuleSaveInput,
    package: &ProtocolPackageRef,
    schema_version: u32,
) -> AppResult<()> {
    if input.package != *package {
        return Err(AppError::new(
            "SOCKET_RULE_PACKAGE_MISMATCH",
            "规则协议包必须与 Listener 的精确绑定一致。",
        ));
    }
    if input.schema_version != schema_version {
        return Err(AppError::new(
            "SOCKET_RULE_SCHEMA_MISMATCH",
            "规则 Schema 版本必须与协议包编译描述一致。",
        ));
    }
    Ok(())
}

fn ensure_immutable_binding(
    current: &SocketDocumentRuleDefinition,
    input: &SocketRuleSaveInput,
) -> AppResult<()> {
    if current.listener_id() == input.listener_id
        && current.package() == &input.package
        && current.schema_version() == input.schema_version
        && current.direction() == input.direction
    {
        return Ok(());
    }
    Err(AppError::new(
        "SOCKET_RULE_BINDING_IMMUTABLE",
        "更新规则不能改变 Listener、协议包、Schema 或方向绑定。",
    )
    .entity(current.rule_id().to_string()))
}

fn validate_rule(
    rule: &SocketDocumentRuleDefinition,
    context: &SocketRuleContext,
) -> AppResult<()> {
    if rule.modifies_document() && !context.can_modify {
        return Err(AppError::new(
            "SOCKET_RULE_ENCODE_REQUIRED",
            "SetField 或 ClearDocument 要求所选方向同时开启并声明 Encode。",
        )
        .entity(rule.rule_id().to_string()));
    }
    rule.validate_against_schema(&context.schema)?;
    Ok(())
}

fn next_created_order(
    rules: &[SocketDocumentRuleDefinition],
    persisted_high_water: u64,
) -> AppResult<u64> {
    rules
        .iter()
        .map(SocketDocumentRuleDefinition::created_order)
        .max()
        .unwrap_or(0)
        .max(persisted_high_water)
        .checked_add(1)
        .filter(|value| (1..=MAX_JAVASCRIPT_SAFE_INTEGER).contains(value))
        .ok_or_else(|| {
            AppError::new(
                "SOCKET_RULE_CREATED_ORDER_EXHAUSTED",
                "Socket 规则创建顺序已达到可表示上限。",
            )
        })
}

fn socket_rule_not_found(rule_id: SocketDocumentRuleId) -> AppError {
    AppError::new(
        "SOCKET_RULE_NOT_FOUND",
        "当前 Workspace 中不存在指定的 Socket 规则。",
    )
    .entity(rule_id.to_string())
}
