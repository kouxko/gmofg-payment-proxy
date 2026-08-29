use intercept_proxy_domain::{
    HttpDocumentRuleContent, HttpRuleContent, ListenerId, Revision, RuleContent, RuleDefinition,
    RuleDefinitionDraft, RuleId, RuleStage, SocketRuleContent,
};

use super::{
    HttpRuleStage, HttpRuleTemplate, ProtocolDocumentRuleTemplate, ProtocolRuleStage, Uuid,
};
use crate::{AppError, AppResult};

impl HttpRuleTemplate {
    pub(crate) fn to_domain(
        &self,
        id: Uuid,
        created_order: u64,
        listener_id: ListenerId,
    ) -> AppResult<RuleDefinition> {
        let priority = i32::try_from(self.priority).map_err(|_| {
            AppError::new(
                "HTTP_RULE_PRIORITY_INVALID",
                "HTTP rule priority is out of range",
            )
        })?;
        RuleDefinition::restore(
            RuleId::from_uuid(id),
            Revision::INITIAL,
            RuleDefinitionDraft {
                name: self.name.clone(),
                enabled: self.enabled,
                priority,
                listener_id,
                stage: self.unified_stage(),
                content: RuleContent::Http(HttpRuleContent {
                    description: self.description.clone(),
                    conditions: self
                        .conditions
                        .clone()
                        .into_iter()
                        .map(Into::into)
                        .collect(),
                    actions: self.actions.clone().into_iter().map(Into::into).collect(),
                    document: self
                        .document
                        .as_ref()
                        .map(super::HttpDocumentRuleTemplate::to_domain)
                        .transpose()?,
                    one_shot: self.one_shot,
                    hit_count: 0,
                    last_hit_at: None,
                }),
            },
            created_order,
        )
        .map_err(AppError::from)
    }

    pub(crate) fn to_domain_existing(
        &self,
        existing: &RuleDefinition,
        listener_id: ListenerId,
    ) -> AppResult<RuleDefinition> {
        if existing.listener_id() != listener_id {
            return Err(selector_error("EXISTING_RULE_ID_BINDING_MISMATCH"));
        }
        let RuleContent::Http(existing_content) = existing.content() else {
            return Err(selector_error("EXISTING_RULE_ID_KIND_MISMATCH"));
        };
        let candidate_binding = self
            .document
            .as_ref()
            .map(super::HttpDocumentRuleTemplate::binding)
            .transpose()?;
        let existing_binding = existing_content
            .document
            .as_ref()
            .map(|document| &document.package);
        match (candidate_binding.as_ref(), existing_binding) {
            (None, None) => {}
            (Some(candidate_package), Some(package)) => {
                if candidate_package != package {
                    return Err(selector_error("EXISTING_RULE_ID_PACKAGE_MISMATCH"));
                }
            }
            _ => return Err(selector_error("EXISTING_RULE_ID_KIND_MISMATCH")),
        }
        self.validate_domain()?;
        let projected = self.to_domain(
            existing.rule_id().as_uuid(),
            existing.created_order(),
            listener_id,
        )?;
        let RuleContent::Http(mut content) = projected.to_draft().content else {
            unreachable!("HTTP projection creates HTTP content")
        };
        content.hit_count = existing_content.hit_count;
        content.last_hit_at = existing_content.last_hit_at;
        let mut draft = projected.to_draft();
        draft.content = RuleContent::Http(content);
        RuleDefinition::restore(
            existing.rule_id(),
            existing.revision(),
            draft,
            existing.created_order(),
        )
        .map_err(AppError::from)
    }

    const fn unified_stage(&self) -> RuleStage {
        match self.stage {
            HttpRuleStage::AppToProxy => RuleStage::AppToProxy,
            HttpRuleStage::ProxyToUpstream => RuleStage::ProxyToUpstream,
            HttpRuleStage::UpstreamToProxy => RuleStage::UpstreamToProxy,
            HttpRuleStage::ProxyToApp => RuleStage::ProxyToApp,
            HttpRuleStage::TlsHandshake => RuleStage::TlsHandshake,
        }
    }
}

impl super::HttpDocumentRuleTemplate {
    fn binding(&self) -> AppResult<intercept_proxy_domain::ProtocolPackageRef> {
        self.package.to_domain()
    }

    fn to_domain(&self) -> AppResult<HttpDocumentRuleContent> {
        Ok(HttpDocumentRuleContent {
            package: self.binding()?,
            conditions: self.conditions.clone(),
            actions: self.actions.clone(),
        })
    }
}

impl ProtocolDocumentRuleTemplate {
    pub(crate) fn to_domain(
        &self,
        id: RuleId,
        created_order: u64,
        listener_id: ListenerId,
        http: bool,
    ) -> AppResult<RuleDefinition> {
        let document = HttpDocumentRuleContent {
            package: self.package.to_domain()?,
            conditions: self.conditions.clone(),
            actions: self.actions.clone(),
        };
        let content = if http {
            RuleContent::Http(HttpRuleContent {
                description: String::new(),
                conditions: Vec::new(),
                actions: Vec::new(),
                document: Some(document),
                one_shot: false,
                hit_count: 0,
                last_hit_at: None,
            })
        } else {
            RuleContent::Socket(SocketRuleContent {
                package: document.package,
                conditions: document.conditions,
                actions: document.actions,
            })
        };
        RuleDefinition::restore(
            id,
            Revision::INITIAL,
            RuleDefinitionDraft {
                name: self.name.clone(),
                enabled: self.enabled,
                priority: self.priority,
                listener_id,
                stage: self.unified_stage(),
                content,
            },
            created_order,
        )
        .map_err(AppError::from)
    }

    pub(crate) fn to_domain_existing(
        &self,
        existing: &RuleDefinition,
        listener_id: ListenerId,
        http: bool,
    ) -> AppResult<RuleDefinition> {
        if existing.listener_id() != listener_id {
            return Err(selector_error("EXISTING_RULE_ID_BINDING_MISMATCH"));
        }
        let package = match existing.content() {
            RuleContent::Http(content) if http => content
                .document
                .as_ref()
                .map(|document| &document.package)
                .ok_or_else(|| selector_error("EXISTING_RULE_ID_KIND_MISMATCH"))?,
            RuleContent::Socket(content) if !http => &content.package,
            _ => return Err(selector_error("EXISTING_RULE_ID_KIND_MISMATCH")),
        };
        if package != &self.package.to_domain()? {
            return Err(selector_error("EXISTING_RULE_ID_PACKAGE_MISMATCH"));
        }
        let projected = self.to_domain(
            existing.rule_id(),
            existing.created_order(),
            listener_id,
            http,
        )?;
        RuleDefinition::restore(
            existing.rule_id(),
            existing.revision(),
            projected.to_draft(),
            existing.created_order(),
        )
        .map_err(AppError::from)
    }

    const fn unified_stage(&self) -> RuleStage {
        match self.stage {
            ProtocolRuleStage::AppToProxy => RuleStage::AppToProxy,
            ProtocolRuleStage::ProxyToUpstream => RuleStage::ProxyToUpstream,
            ProtocolRuleStage::UpstreamToProxy => RuleStage::UpstreamToProxy,
            ProtocolRuleStage::ProxyToApp => RuleStage::ProxyToApp,
        }
    }
}

fn selector_error(code: &'static str) -> AppError {
    AppError::new(code, "existing rule selector validation failed")
}
