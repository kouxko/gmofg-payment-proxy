//! Workspace 与完整配置导入时的协议包引用校验。
//!
//! Wire 层只负责保证内嵌文件集合有界且身份集合正确；本模块在任何证书恢复或数据库写入
//! 之前，使用真实编译描述交叉校验 Listener 的方向能力与 协议 Document 规则 Schema。
//! 这样即使 portability adapter 错误返回了串包描述，导入仍会 fail-closed。

use std::collections::{HashMap, HashSet};

use intercept_proxy_domain::{
    DocumentField, DocumentFieldType, DocumentSchema, DocumentSchemaId, ListenerDataPlane,
    ProtocolDirection, ProtocolPackageRef, ProxyWorkspace,
};

use super::protocol_packages::ensure_description_identity;
use crate::{
    AppError, AppResult, ProtocolPackageDescriptionViewModel, ProtocolPackageKindViewModel,
    ProtocolPackageSchemaFieldTypeViewModel, ProtocolPackageSchemaViewModel,
};

/// 校验 fresh portability 编译描述与待提交聚合的一致性。
///
/// 这是无状态的原子提交门禁：Facade 在恢复证书前调用一次，Infrastructure 在事务写入
/// 前对重新编译的结果再调用一次。`expected_packages` 必须是文档声明/实际引用的精确集合，
/// 从而拒绝串包描述、缺失/额外描述、方向能力不足以及规则 Schema 类型漂移。
#[doc(hidden)]
pub fn validate_portable_protocol_bindings(
    workspaces: &[ProxyWorkspace],
    expected_packages: &[ProtocolPackageRef],
    descriptions: &[ProtocolPackageDescriptionViewModel],
) -> AppResult<()> {
    let expected = expected_packages.iter().cloned().collect::<HashSet<_>>();
    if expected.len() != expected_packages.len() || descriptions.len() != expected.len() {
        return Err(portability_error(
            "协议包预检描述数量与文档中的精确包集合不一致。",
        ));
    }

    let mut by_package = HashMap::with_capacity(descriptions.len());
    for description in descriptions {
        if !expected.contains(&description.package)
            || by_package
                .insert(description.package.clone(), description)
                .is_some()
        {
            return Err(portability_error(
                "协议包预检返回了重复、缺失或不属于当前文档的精确身份。",
            ));
        }
    }
    if by_package.len() != expected.len() {
        return Err(portability_error("协议包预检缺少精确版本描述。"));
    }

    for workspace in workspaces {
        workspace.validate().map_err(AppError::from)?;
        validate_workspace_bindings(workspace, &by_package)?;
    }
    Ok(())
}

/// 校验一个选择协议包的入口与它当前绑定的全部协议报文规则。
///
/// 入口保存/启动只应重验目标入口，不能因为同一 Workspace 中另一个未修改入口的
/// 外部包状态而阻断当前操作。调用方已经用精确包身份取得 fresh 编译描述；这里继续
/// fail-closed 校验描述身份、双方向入口能力、规则 Schema/方向以及规则与入口的
/// 精确版本绑定。HTTP Plain 与 Socket Direct 没有协议包边界，直接返回成功。
pub(super) fn validate_listener_protocol_binding(
    workspace: &ProxyWorkspace,
    listener_id: crate::ListenerId,
    description: &ProtocolPackageDescriptionViewModel,
) -> AppResult<()> {
    let listener = workspace
        .listeners
        .iter()
        .find(|listener| listener.id == listener_id)
        .ok_or_else(|| portability_error("待校验的 Listener 不存在。"))?;
    let Some(package) = crate::listener_protocol_package(listener) else {
        return Ok(());
    };
    ensure_description_identity(package, description)?;
    ensure_kind(&listener.data_plane, description.kind)?;
    for rule in workspace
        .document_runtime_rules()?
        .into_iter()
        .filter(|rule| rule.listener_id() == listener_id)
    {
        validate_rule_binding(package, &rule, description)?;
    }
    Ok(())
}

fn validate_workspace_bindings(
    workspace: &ProxyWorkspace,
    descriptions: &HashMap<ProtocolPackageRef, &ProtocolPackageDescriptionViewModel>,
) -> AppResult<()> {
    for listener in &workspace.listeners {
        let Some(package) = crate::listener_protocol_package(listener) else {
            continue;
        };
        let description = required_description(descriptions, package)?;
        ensure_description_identity(package, description)?;
        ensure_kind(&listener.data_plane, description.kind)?;
    }

    for rule in workspace.document_runtime_rules()? {
        let description = required_description(descriptions, rule.package())?;
        ensure_description_identity(rule.package(), description)?;
        let listener = workspace
            .listeners
            .iter()
            .find(|listener| listener.id == rule.listener_id())
            .ok_or_else(|| portability_error("协议报文规则引用的 Listener 不存在。"))?;
        let package = crate::listener_protocol_package(listener)
            .ok_or_else(|| portability_error("报文规则只能绑定已选择协议方案的入口。"))?;
        ensure_kind(&listener.data_plane, description.kind)?;
        validate_rule_binding(package, &rule, description)?;
    }
    Ok(())
}

fn validate_rule_binding(
    package: &ProtocolPackageRef,
    rule: &intercept_proxy_domain::ProtocolDocumentRuleDefinition,
    description: &ProtocolPackageDescriptionViewModel,
) -> AppResult<()> {
    let schema = domain_schema(schema_for_direction(description, rule.direction()))?;
    rule.validate_against_schema(&schema)?;
    if package != rule.package() {
        return Err(portability_error("规则与入口绑定的精确协议包不一致。"));
    }
    Ok(())
}

fn required_description<'a>(
    descriptions: &'a HashMap<ProtocolPackageRef, &ProtocolPackageDescriptionViewModel>,
    package: &ProtocolPackageRef,
) -> AppResult<&'a ProtocolPackageDescriptionViewModel> {
    descriptions.get(package).copied().ok_or_else(|| {
        portability_error("文档引用的精确协议包没有通过预检。")
            .entity(format!("{}@{}", package.id, package.version))
    })
}

fn ensure_kind(
    data_plane: &ListenerDataPlane,
    kind: ProtocolPackageKindViewModel,
) -> AppResult<()> {
    if matches!(
        (data_plane, kind),
        (
            ListenerDataPlane::Http(_),
            ProtocolPackageKindViewModel::Http
        ) | (
            ListenerDataPlane::Socket(_),
            ProtocolPackageKindViewModel::Socket
        )
    ) {
        Ok(())
    } else {
        Err(portability_error("协议包类型与入口数据平面不一致。"))
    }
}

fn schema_for_direction(
    description: &ProtocolPackageDescriptionViewModel,
    direction: ProtocolDirection,
) -> &ProtocolPackageSchemaViewModel {
    match direction {
        ProtocolDirection::Upstream => &description.upstream_schema,
        ProtocolDirection::Downstream => &description.downstream_schema,
    }
}

fn domain_schema(schema: &ProtocolPackageSchemaViewModel) -> AppResult<DocumentSchema> {
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

fn portability_error(message: impl Into<String>) -> AppError {
    AppError::new("PORTABLE_PROTOCOL_PACKAGE_INVALID", message)
}
