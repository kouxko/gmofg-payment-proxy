use super::HttpRuleTemplate;
use crate::{AppError, AppResult, ListenerId};
use uuid::Uuid;

impl HttpRuleTemplate {
    pub(crate) fn validate_domain(&self) -> AppResult<()> {
        self.to_domain(Uuid::nil(), 1, ListenerId::new())
            .map(|_| ())
            .map_err(|_| invalid_rule())
    }
}

fn invalid_rule() -> AppError {
    AppError::new("HTTP_RULE_INVALID", "HTTP rule domain validation failed")
}
