use intercept_proxy_domain::{
    ConditionTree, HttpAction, HttpDocumentRuleContent, HttpRuleContent, ListenerId, Revision,
    RuleContent, RuleDefinition, RuleDefinitionDraft, RuleDefinitionRestoreSnapshot, RuleId,
    RuleLifecycle, RuleStage, SocketRuleContent, UnifiedAction,
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
        let definition = RuleDefinition::restore(
            RuleId::from_uuid(id),
            RuleDefinitionDraft {
                name: self.name.clone(),
                enabled: self.enabled,
                priority,
                listener_id,
                stage: self.unified_stage(),
                one_shot: self.one_shot,
                content: RuleContent::Http(HttpRuleContent {
                    description: self.description.clone(),
                    condition: ConditionTree::All(
                        self.conditions
                            .clone()
                            .into_iter()
                            .map(Into::into)
                            .map(intercept_proxy_domain::ConditionTree::Leaf)
                            .chain(self.document.iter().flat_map(|document| {
                                match ConditionTree::from_document_conditions(
                                    document.conditions.clone(),
                                ) {
                                    ConditionTree::All(children) => children,
                                    tree => vec![tree],
                                }
                            }))
                            .collect(),
                    ),
                    actions: self
                        .actions
                        .clone()
                        .into_iter()
                        .map(HttpAction::from)
                        .map(UnifiedAction::from)
                        .chain(self.document.iter().flat_map(|document| {
                            document
                                .actions
                                .clone()
                                .into_iter()
                                .map(UnifiedAction::from)
                        }))
                        .collect(),
                    document: self
                        .document
                        .as_ref()
                        .map(super::HttpDocumentRuleTemplate::to_domain)
                        .transpose()?,
                }),
            },
            RuleDefinitionRestoreSnapshot {
                revision: Revision::INITIAL,
                created_order,
                lifecycle: RuleLifecycle::default(),
            },
        )
        .map_err(AppError::from)
        .map_err(http_rule_invalid)?;
        definition
            .validate_for_save()
            .map_err(AppError::from)
            .map_err(http_rule_invalid)?;
        Ok(definition)
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
        let draft = projected.to_draft();
        let definition = RuleDefinition::restore(
            existing.rule_id(),
            draft,
            RuleDefinitionRestoreSnapshot {
                revision: existing.revision(),
                created_order: existing.created_order(),
                lifecycle: existing.lifecycle().clone(),
            },
        )
        .map_err(AppError::from)
        .map_err(http_rule_invalid)?;
        definition
            .validate_for_save()
            .map_err(AppError::from)
            .map_err(http_rule_invalid)?;
        Ok(definition)
    }

    const fn unified_stage(&self) -> RuleStage {
        match self.stage {
            HttpRuleStage::ProxyToUpstream => RuleStage::ProxyToUpstream,
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
        };
        let condition = ConditionTree::from_document_conditions(self.conditions.clone());
        let actions = self
            .actions
            .clone()
            .into_iter()
            .map(UnifiedAction::from)
            .collect();
        let content = if http {
            RuleContent::Http(HttpRuleContent {
                description: String::new(),
                condition,
                actions,
                document: Some(document),
            })
        } else {
            RuleContent::Socket(SocketRuleContent {
                package: document.package,
                condition,
                actions,
            })
        };
        let definition = RuleDefinition::restore(
            id,
            RuleDefinitionDraft {
                name: self.name.clone(),
                enabled: self.enabled,
                priority: self.priority,
                listener_id,
                stage: self.unified_stage(),
                one_shot: false,
                content,
            },
            RuleDefinitionRestoreSnapshot {
                revision: Revision::INITIAL,
                created_order,
                lifecycle: RuleLifecycle::default(),
            },
        )
        .map_err(AppError::from)
        .map_err(protocol_document_rule_invalid)?;
        definition
            .validate_for_save()
            .map_err(AppError::from)
            .map_err(protocol_document_rule_invalid)?;
        Ok(definition)
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
        let definition = RuleDefinition::restore(
            existing.rule_id(),
            projected.to_draft(),
            RuleDefinitionRestoreSnapshot {
                revision: existing.revision(),
                created_order: existing.created_order(),
                lifecycle: existing.lifecycle().clone(),
            },
        )
        .map_err(AppError::from)
        .map_err(protocol_document_rule_invalid)?;
        definition
            .validate_for_save()
            .map_err(AppError::from)
            .map_err(protocol_document_rule_invalid)?;
        Ok(definition)
    }

    const fn unified_stage(&self) -> RuleStage {
        match self.stage {
            ProtocolRuleStage::ProxyToUpstream => RuleStage::ProxyToUpstream,
            ProtocolRuleStage::ProxyToApp => RuleStage::ProxyToApp,
        }
    }
}

fn selector_error(code: &'static str) -> AppError {
    AppError::new(code, "existing rule selector validation failed")
}

fn http_rule_invalid(_: AppError) -> AppError {
    AppError::new("HTTP_RULE_INVALID", "HTTP rule domain validation failed")
}

fn protocol_document_rule_invalid(_: AppError) -> AppError {
    AppError::new(
        "PROTOCOL_DOCUMENT_RULE_INVALID",
        "protocol Document rule domain validation failed",
    )
}
