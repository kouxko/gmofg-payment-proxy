use intercept_proxy_domain::{
    ListenerId, Revision, RuleContent, RuleDefinition, RuleDefinitionDraft,
    RuleDefinitionRestoreSnapshot, RuleId, RuleLifecycle,
};

use super::RuleTemplate;
use crate::{AppError, AppResult};

impl RuleTemplate {
    pub(crate) fn to_domain(
        &self,
        id: RuleId,
        created_order: u64,
        listener_id: ListenerId,
    ) -> AppResult<RuleDefinition> {
        restore_rule(
            id,
            created_order,
            Revision::INITIAL,
            RuleLifecycle::default(),
            listener_id,
            self,
        )
    }

    pub(crate) fn to_domain_existing(
        &self,
        existing: &RuleDefinition,
        listener_id: ListenerId,
    ) -> AppResult<RuleDefinition> {
        validate_immutable_binding(existing.content(), &self.content)?;
        if existing.listener_id() != listener_id {
            return Err(selector_error("EXISTING_RULE_ID_BINDING_MISMATCH"));
        }
        restore_rule(
            existing.rule_id(),
            existing.created_order(),
            existing.revision(),
            existing.lifecycle().clone(),
            listener_id,
            self,
        )
    }

    pub(crate) fn validate_domain(&self) -> AppResult<()> {
        self.to_domain(RuleId::from_uuid(uuid::Uuid::nil()), 1, ListenerId::new())
            .map(|_| ())
    }
}

fn restore_rule(
    id: RuleId,
    created_order: u64,
    revision: Revision,
    lifecycle: RuleLifecycle,
    listener_id: ListenerId,
    template: &RuleTemplate,
) -> AppResult<RuleDefinition> {
    let definition = RuleDefinition::restore(
        id,
        RuleDefinitionDraft {
            name: template.name.clone(),
            enabled: template.enabled,
            priority: template.priority,
            listener_id,
            stage: template.stage,
            content: template.content.clone(),
        },
        RuleDefinitionRestoreSnapshot {
            revision,
            created_order,
            lifecycle,
        },
    )
    .map_err(AppError::from)
    .map_err(|_| rule_invalid(&template.content))?;
    definition
        .validate_for_save()
        .map_err(AppError::from)
        .map_err(|_| rule_invalid(&template.content))?;
    Ok(definition)
}

fn validate_immutable_binding(existing: &RuleContent, candidate: &RuleContent) -> AppResult<()> {
    match (existing, candidate) {
        (RuleContent::Http(_), RuleContent::Http(_)) => Ok(()),
        (RuleContent::Socket(existing), RuleContent::Socket(candidate))
            if existing.package == candidate.package =>
        {
            Ok(())
        }
        (RuleContent::Socket(_), RuleContent::Socket(_)) => {
            Err(selector_error("EXISTING_RULE_ID_PACKAGE_MISMATCH"))
        }
        _ => Err(selector_error("EXISTING_RULE_ID_KIND_MISMATCH")),
    }
}

fn selector_error(code: &'static str) -> AppError {
    AppError::new(code, "existing rule selector validation failed")
}

fn rule_invalid(content: &RuleContent) -> AppError {
    match content {
        RuleContent::Http(_) => AppError::new("HTTP_RULE_INVALID", "HTTP rule validation failed"),
        RuleContent::Socket(_) => AppError::new(
            "PROTOCOL_DOCUMENT_RULE_INVALID",
            "Socket rule validation failed",
        ),
    }
}
