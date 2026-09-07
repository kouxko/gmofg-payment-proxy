//! HTTP 规则、Encode 与 Finalize 的联合事务端口。

use async_trait::async_trait;
use intercept_proxy_exchange::{Error, HttpAction, MatchContext};
use uuid::Uuid;

use super::Finalize;
use crate::JointRuleConditionEvaluation;

#[async_trait]
pub trait HttpJointEvaluation: Finalize + Send + Sync {
    fn gate(
        &mut self,
        rule_id: Uuid,
        context: &MatchContext<'_>,
        message: &mut crate::Message,
        apply_action: &mut dyn FnMut(&mut crate::Message, &HttpAction) -> crate::Result<()>,
    ) -> crate::Result<JointRuleConditionEvaluation>;

    async fn encode(&mut self, message: &mut crate::Message) -> Result<(), Error>;
}
