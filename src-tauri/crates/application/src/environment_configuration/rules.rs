use intercept_proxy_domain::{RuleContent, RuleStage};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

mod projection;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuleTemplate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) existing_rule_id: Option<Uuid>,
    pub(super) name: String,
    pub(super) enabled: bool,
    pub(super) priority: i32,
    pub(super) listener_alias: String,
    pub(super) stage: RuleStage,
    pub(super) one_shot: bool,
    pub(super) content: RuleContent,
}

impl RuleTemplate {
    pub(super) const fn existing_rule_id(&self) -> Option<Uuid> {
        self.existing_rule_id
    }

    pub(super) fn listener_alias(&self) -> &str {
        &self.listener_alias
    }

    pub(super) const fn is_http(&self) -> bool {
        matches!(self.content, RuleContent::Http(_))
    }
}
