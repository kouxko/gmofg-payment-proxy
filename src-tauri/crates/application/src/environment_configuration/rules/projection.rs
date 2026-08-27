use intercept_proxy_domain::{
    ChannelId, ListenerId, ProtocolDocumentRuleDefinition, ProtocolDocumentRuleId,
    ProtocolRuleStage as DomainProtocolRuleStage, Revision, Rule, RuleId,
};

use super::{HttpRuleTemplate, ProtocolDocumentRuleTemplate, ProtocolRuleStage, Uuid};
use crate::{AppError, AppResult};

impl HttpRuleTemplate {
    pub(crate) fn to_domain(
        &self,
        id: Uuid,
        created_order: u64,
        listener_id: ListenerId,
    ) -> AppResult<Rule> {
        Ok(Rule {
            id: RuleId::from_uuid(id),
            revision: Revision::INITIAL,
            name: self.name.clone(),
            description: self.description.clone(),
            enabled: self.enabled,
            priority: self.priority,
            created_order,
            channel: Some(ChannelId::new(listener_id.to_string()).map_err(AppError::from)?),
            stage: self.domain_stage(),
            conditions: self
                .conditions
                .clone()
                .into_iter()
                .map(Into::into)
                .collect(),
            actions: self.actions.clone().into_iter().map(Into::into).collect(),
            one_shot: self.one_shot,
            hit_count: 0,
            last_hit_at: None,
        })
    }

    pub(crate) fn to_domain_existing(
        &self,
        existing: &Rule,
        listener_id: ListenerId,
    ) -> AppResult<Rule> {
        let expected_channel = listener_id.to_string();
        if existing.stage != self.domain_stage() {
            return Err(selector_error("EXISTING_RULE_ID_STAGE_MISMATCH"));
        }
        if existing.channel.as_ref().map(ChannelId::as_str) != Some(expected_channel.as_str()) {
            return Err(selector_error("EXISTING_RULE_ID_BINDING_MISMATCH"));
        }
        self.validate_domain()?;
        let mut projected =
            self.to_domain(existing.id.as_uuid(), existing.created_order, listener_id)?;
        projected.revision = existing.revision;
        projected.hit_count = existing.hit_count;
        projected.last_hit_at = existing.last_hit_at;
        Ok(projected)
    }
}

impl ProtocolDocumentRuleTemplate {
    pub(crate) fn to_domain(
        &self,
        id: ProtocolDocumentRuleId,
        created_order: u64,
        listener_id: ListenerId,
    ) -> AppResult<ProtocolDocumentRuleDefinition> {
        ProtocolDocumentRuleDefinition::new_named_for_stage(
            id,
            self.name.clone(),
            self.enabled,
            self.priority,
            created_order,
            listener_id,
            self.package.to_domain()?,
            self.schema_version,
            match self.stage {
                ProtocolRuleStage::AppToProxy => DomainProtocolRuleStage::AppToProxy,
                ProtocolRuleStage::ProxyToUpstream => DomainProtocolRuleStage::ProxyToUpstream,
                ProtocolRuleStage::UpstreamToProxy => DomainProtocolRuleStage::UpstreamToProxy,
                ProtocolRuleStage::ProxyToApp => DomainProtocolRuleStage::ProxyToApp,
            },
            self.conditions.clone(),
            self.actions.clone(),
        )
        .map_err(|_| {
            AppError::new(
                "PROTOCOL_DOCUMENT_RULE_INVALID",
                "protocol Document rule domain validation failed",
            )
        })
    }

    pub(crate) fn to_domain_existing(
        &self,
        existing: &ProtocolDocumentRuleDefinition,
        listener_id: ListenerId,
    ) -> AppResult<ProtocolDocumentRuleDefinition> {
        if existing.listener_id() != listener_id {
            return Err(selector_error("EXISTING_RULE_ID_BINDING_MISMATCH"));
        }
        let package = self.package.to_domain()?;
        if existing.package() != &package {
            return Err(selector_error("EXISTING_RULE_ID_PACKAGE_MISMATCH"));
        }
        if existing.schema_version() != self.schema_version {
            return Err(selector_error("EXISTING_RULE_ID_SCHEMA_VERSION_MISMATCH"));
        }
        let stage = match self.stage {
            ProtocolRuleStage::AppToProxy => DomainProtocolRuleStage::AppToProxy,
            ProtocolRuleStage::ProxyToUpstream => DomainProtocolRuleStage::ProxyToUpstream,
            ProtocolRuleStage::UpstreamToProxy => DomainProtocolRuleStage::UpstreamToProxy,
            ProtocolRuleStage::ProxyToApp => DomainProtocolRuleStage::ProxyToApp,
        };
        if existing.stage() != stage {
            return Err(selector_error("EXISTING_RULE_ID_STAGE_MISMATCH"));
        }
        self.to_domain(existing.rule_id(), existing.created_order(), listener_id)
    }
}

fn selector_error(code: &'static str) -> AppError {
    AppError::new(code, "existing rule selector validation failed")
}
