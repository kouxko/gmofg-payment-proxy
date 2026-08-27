use intercept_proxy_domain::{RuleDraft, validate_rule_draft};

use super::HttpRuleTemplate;
use crate::{AppError, AppResult};

impl HttpRuleTemplate {
    pub(crate) fn validate_domain(&self) -> AppResult<()> {
        let draft = RuleDraft {
            expected_revision: None,
            name: self.name.clone(),
            description: self.description.clone(),
            enabled: self.enabled,
            priority: self.priority,
            created_order: 1,
            channel: None,
            stage: self.domain_stage(),
            conditions: self
                .conditions
                .clone()
                .into_iter()
                .map(Into::into)
                .collect(),
            actions: self.actions.clone().into_iter().map(Into::into).collect(),
            one_shot: self.one_shot,
        };
        validate_rule_draft(&draft).map_err(|_| invalid_rule())
    }
}

fn invalid_rule() -> AppError {
    AppError::new("HTTP_RULE_INVALID", "HTTP rule domain validation failed")
}
