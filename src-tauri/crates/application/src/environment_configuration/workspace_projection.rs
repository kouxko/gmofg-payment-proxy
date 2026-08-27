use std::collections::BTreeMap;

use intercept_proxy_domain::{ListenerId, ProxyWorkspace, Revision};

use super::identity::EnvironmentIdentityAllocatorPort;
use super::{EnvironmentConfigurationCandidateV1, EnvironmentTarget};
use crate::{AppError, AppResult};

pub(crate) fn project_candidate_workspace(
    candidate: &EnvironmentConfigurationCandidateV1,
    persisted: Option<&ProxyWorkspace>,
    workspace_scope: &[ProxyWorkspace],
    allocator: &dyn EnvironmentIdentityAllocatorPort,
    checkpoint: &dyn super::EnvironmentValidationCheckpoint,
) -> AppResult<ProxyWorkspace> {
    ensure_running(checkpoint)?;
    let (id, name, revision) = match (&candidate.target, persisted) {
        (EnvironmentTarget::New { name }, None) => (
            allocator.allocate_workspace_id(),
            name.trim().to_owned(),
            Revision::INITIAL,
        ),
        (EnvironmentTarget::Existing { workspace_id, .. }, Some(workspace))
            if workspace.id.as_uuid() == *workspace_id =>
        {
            (workspace.id, workspace.name.clone(), workspace.revision)
        }
        _ => return Err(projection_error()),
    };

    let (listener_ids, listeners) = project_listeners(candidate, persisted, allocator, checkpoint)?;

    let rules = candidate
        .workspace
        .http_rules
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            ensure_running(checkpoint)?;
            let listener_id = required_listener(
                candidate,
                &listener_ids,
                rule.listener_alias(),
                RuleListenerKind::Http,
            )?;
            if let Some(id) = rule.existing_rule_id {
                reconcile_http_rule(
                    rule,
                    id,
                    listener_id,
                    persisted,
                    workspace_scope,
                    checkpoint,
                )
            } else {
                let (id, created_order) = allocator.allocate_http_rule(index);
                rule.to_domain(id, created_order, listener_id)
            }
        })
        .collect::<AppResult<Vec<_>>>()?;
    let protocol_rules = candidate
        .workspace
        .protocol_rules
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            ensure_running(checkpoint)?;
            let listener_id = required_listener(
                candidate,
                &listener_ids,
                rule.listener_alias(),
                RuleListenerKind::Protocol,
            )?;
            if let Some(id) = rule.existing_rule_id {
                reconcile_protocol_rule(
                    rule,
                    id,
                    listener_id,
                    persisted,
                    workspace_scope,
                    checkpoint,
                )
            } else {
                let (id, created_order) = allocator.allocate_protocol_rule(index);
                rule.to_domain(id, created_order, listener_id)
            }
        })
        .collect::<AppResult<Vec<_>>>()?;
    let protocol_rule_created_order_high_water = protocol_created_order_high_water(&protocol_rules);
    let android_network_profiles = candidate
        .workspace
        .android_network_profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            ensure_running(checkpoint)?;
            let profile_id = profile.id().map_or_else(
                || allocator.allocate_android_profile_id(index),
                str::to_owned,
            );
            profile.to_domain(profile_id, &listener_ids)
        })
        .collect::<AppResult<Vec<_>>>()?;

    assemble_workspace(ProxyWorkspace {
        id,
        name,
        revision,
        listeners,
        rules,
        protocol_rules,
        protocol_rule_created_order_high_water,
        certificate_references: Vec::new(),
        android_network_profiles,
    })
}

fn assemble_workspace(workspace: ProxyWorkspace) -> AppResult<ProxyWorkspace> {
    workspace.validate().map_err(AppError::from)?;
    Ok(workspace)
}

fn protocol_created_order_high_water(
    rules: &[intercept_proxy_domain::ProtocolDocumentRuleDefinition],
) -> u64 {
    rules
        .iter()
        .map(intercept_proxy_domain::ProtocolDocumentRuleDefinition::created_order)
        .max()
        .unwrap_or(0)
}

fn project_listeners<'a>(
    candidate: &'a EnvironmentConfigurationCandidateV1,
    persisted: Option<&ProxyWorkspace>,
    allocator: &dyn EnvironmentIdentityAllocatorPort,
    checkpoint: &dyn super::EnvironmentValidationCheckpoint,
) -> AppResult<(BTreeMap<&'a str, ListenerId>, Vec<crate::ProxyListener>)> {
    let mut listener_ids = BTreeMap::new();
    let mut listeners = Vec::with_capacity(candidate.workspace.listeners.len());
    for (index, listener) in candidate.workspace.listeners.iter().enumerate() {
        ensure_running(checkpoint)?;
        let listener_id = match (listener.id, persisted) {
            (Some(id), Some(workspace)) if listener_belongs(workspace, id, checkpoint)? => {
                ListenerId::from_uuid(id)
            }
            (Some(_), _) => {
                return Err(AppError::new(
                    "LISTENER_DOMAIN_INVALID",
                    "existing listener does not belong to the target Workspace",
                ));
            }
            (None, _) => allocator.allocate_listener_id(index, listener.alias()),
        };
        if listener_ids.insert(listener.alias(), listener_id).is_some() {
            return Err(AppError::new(
                "LISTENER_ALIAS_DUPLICATE",
                "listener alias graph validation failed",
            ));
        }
        listeners.push(listener.to_domain(listener_id)?);
    }
    Ok((listener_ids, listeners))
}

fn reconcile_http_rule(
    template: &super::HttpRuleTemplate,
    selector: uuid::Uuid,
    listener_id: ListenerId,
    persisted: Option<&ProxyWorkspace>,
    workspace_scope: &[ProxyWorkspace],
    checkpoint: &dyn super::EnvironmentValidationCheckpoint,
) -> AppResult<intercept_proxy_domain::Rule> {
    let Some(workspace) = persisted else {
        return Err(selector_error("EXISTING_RULE_ID_FORBIDDEN"));
    };
    if find_protocol_rule(workspace, selector, checkpoint)?.is_some() {
        return Err(selector_error("EXISTING_RULE_ID_KIND_MISMATCH"));
    }
    if let Some(existing) = find_http_rule(workspace, selector, checkpoint)? {
        return template.to_domain_existing(existing, listener_id);
    }
    if rule_exists_outside(workspace.id, selector, workspace_scope, checkpoint)? {
        return Err(selector_error("EXISTING_RULE_ID_WORKSPACE_MISMATCH"));
    }
    Err(selector_error("EXISTING_RULE_ID_UNKNOWN"))
}

fn reconcile_protocol_rule(
    template: &super::ProtocolDocumentRuleTemplate,
    selector: uuid::Uuid,
    listener_id: ListenerId,
    persisted: Option<&ProxyWorkspace>,
    workspace_scope: &[ProxyWorkspace],
    checkpoint: &dyn super::EnvironmentValidationCheckpoint,
) -> AppResult<intercept_proxy_domain::ProtocolDocumentRuleDefinition> {
    let Some(workspace) = persisted else {
        return Err(selector_error("EXISTING_RULE_ID_FORBIDDEN"));
    };
    if find_http_rule(workspace, selector, checkpoint)?.is_some() {
        return Err(selector_error("EXISTING_RULE_ID_KIND_MISMATCH"));
    }
    if let Some(existing) = find_protocol_rule(workspace, selector, checkpoint)? {
        return template.to_domain_existing(existing, listener_id);
    }
    if rule_exists_outside(workspace.id, selector, workspace_scope, checkpoint)? {
        return Err(selector_error("EXISTING_RULE_ID_WORKSPACE_MISMATCH"));
    }
    Err(selector_error("EXISTING_RULE_ID_UNKNOWN"))
}

fn rule_exists_outside(
    selected_workspace_id: crate::WorkspaceId,
    selector: uuid::Uuid,
    workspace_scope: &[ProxyWorkspace],
    checkpoint: &dyn super::EnvironmentValidationCheckpoint,
) -> AppResult<bool> {
    for workspace in workspace_scope {
        ensure_running(checkpoint)?;
        if workspace.id != selected_workspace_id
            && (find_http_rule(workspace, selector, checkpoint)?.is_some()
                || find_protocol_rule(workspace, selector, checkpoint)?.is_some())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn listener_belongs(
    workspace: &ProxyWorkspace,
    selector: uuid::Uuid,
    checkpoint: &dyn super::EnvironmentValidationCheckpoint,
) -> AppResult<bool> {
    for listener in &workspace.listeners {
        ensure_running(checkpoint)?;
        if listener.id.as_uuid() == selector {
            return Ok(true);
        }
    }
    Ok(false)
}

fn find_http_rule<'a>(
    workspace: &'a ProxyWorkspace,
    selector: uuid::Uuid,
    checkpoint: &dyn super::EnvironmentValidationCheckpoint,
) -> AppResult<Option<&'a intercept_proxy_domain::Rule>> {
    for rule in &workspace.rules {
        ensure_running(checkpoint)?;
        if rule.id.as_uuid() == selector {
            return Ok(Some(rule));
        }
    }
    Ok(None)
}

fn find_protocol_rule<'a>(
    workspace: &'a ProxyWorkspace,
    selector: uuid::Uuid,
    checkpoint: &dyn super::EnvironmentValidationCheckpoint,
) -> AppResult<Option<&'a intercept_proxy_domain::ProtocolDocumentRuleDefinition>> {
    for rule in &workspace.protocol_rules {
        ensure_running(checkpoint)?;
        if rule.rule_id().as_uuid() == selector {
            return Ok(Some(rule));
        }
    }
    Ok(None)
}

fn ensure_running(checkpoint: &dyn super::EnvironmentValidationCheckpoint) -> AppResult<()> {
    if checkpoint.checkpoint() {
        Err(AppError::new(
            "VALIDATION_INTERRUPTED",
            "environment validation interrupted",
        ))
    } else {
        Ok(())
    }
}

fn selector_error(code: &'static str) -> AppError {
    AppError::new(code, "existing rule selector validation failed")
}

#[derive(Clone, Copy)]
enum RuleListenerKind {
    Http,
    Protocol,
}

fn required_listener(
    candidate: &EnvironmentConfigurationCandidateV1,
    listeners: &BTreeMap<&str, ListenerId>,
    alias: &str,
    kind: RuleListenerKind,
) -> AppResult<ListenerId> {
    let listener_id = listeners.get(alias).copied().ok_or_else(|| {
        AppError::new(
            "LISTENER_ALIAS_MISSING",
            "listener alias graph validation failed",
        )
    })?;
    let listener = candidate
        .workspace
        .listeners
        .iter()
        .find(|listener| listener.alias() == alias)
        .ok_or_else(|| {
            AppError::new(
                "LISTENER_ALIAS_MISSING",
                "listener alias graph validation failed",
            )
        })?;
    let compatible = match kind {
        RuleListenerKind::Http => listener.accepts_http_rules(),
        RuleListenerKind::Protocol => listener.accepts_protocol_rules(),
    };
    if !compatible {
        return Err(AppError::new(
            "LISTENER_ALIAS_TYPE_MISMATCH",
            "listener alias type validation failed",
        ));
    }
    Ok(listener_id)
}

fn projection_error() -> AppError {
    AppError::new(
        "VALIDATION_LAYER_FAILED",
        "candidate Workspace projection failed",
    )
}
