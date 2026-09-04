//! HTTP 规则编辑能力矩阵。
//!
//! 这里是 UI 可选项的 Rust 真相源；领域层保存校验仍是最终防线。两层共同保证前端
//! 不展示不可能的组合，旧客户端或损坏输入也无法绕过业务约束。

use super::Application;
use crate::{
    RuleActionCapabilityViewModel, RuleActionKind, RuleMatchFieldCapabilityViewModel,
    RuleMatchFieldKind, RuleMatchOperatorKind, RuleStage, RuleStageCapabilityViewModel,
    RuleTrafficDirection,
};

impl Application {
    #[must_use]
    pub fn rule_capabilities(&self) -> Vec<RuleStageCapabilityViewModel> {
        [RuleStage::ProxyToUpstream, RuleStage::ProxyToApp]
            .into_iter()
            .map(stage_capability)
            .collect()
    }
}

pub(super) fn action_capability(
    stage: RuleStage,
    kind: RuleActionKind,
) -> Option<RuleActionCapabilityViewModel> {
    stage_capability(stage)
        .actions
        .into_iter()
        .find(|capability| capability.kind == kind)
}

pub(super) fn stage_capability(stage: RuleStage) -> RuleStageCapabilityViewModel {
    use RuleActionKind as Action;
    use RuleMatchFieldKind as Field;

    use RuleMatchOperatorKind as Operator;
    let string_operators = vec![
        Operator::Equals,
        Operator::Contains,
        Operator::StartsWith,
        Operator::EndsWith,
        Operator::Wildcard,
    ];
    let capability = |kind, operators, selector| RuleMatchFieldCapabilityViewModel {
        kind,
        operators,
        selector,
    };
    let match_fields = vec![
        capability(Field::Method, vec![Operator::Equals], None),
        capability(Field::RequestTarget, string_operators, None),
    ];
    let common = [
        Action::ReplaceBodyText,
        Action::Delay,
        Action::Jitter,
        Action::Throttle,
        Action::Intermittent,
    ];
    let kinds: Vec<(RuleActionKind, bool)> = match stage {
        RuleStage::ProxyToUpstream => common
            .into_iter()
            .map(|kind| (kind, false))
            .chain([
                (Action::DisconnectBeforeUpstream, true),
                (Action::UpstreamConnectTimeout, true),
                (Action::UpstreamWriteTimeout, true),
                (Action::UpstreamReadTimeout, true),
                (Action::DropUpstreamResponse, true),
                (Action::DisconnectDuringUpstreamWrite, true),
            ])
            .collect(),
        RuleStage::ProxyToApp => common
            .into_iter()
            .map(|kind| (kind, false))
            .chain([
                (Action::CustomHttpStatus, false),
                (Action::InvalidJson, true),
                (Action::IncorrectContentLength, true),
                (Action::TruncateResponse, true),
                (Action::DisconnectDuringDownstreamWrite, true),
            ])
            .collect(),
    };
    let traffic_direction = match stage {
        RuleStage::ProxyToUpstream => RuleTrafficDirection::Upstream,
        RuleStage::ProxyToApp => RuleTrafficDirection::Downstream,
    };
    RuleStageCapabilityViewModel {
        stage,
        match_fields,
        actions: kinds
            .into_iter()
            .map(|(kind, terminal)| RuleActionCapabilityViewModel {
                traffic_direction: matches!(kind, Action::Throttle | Action::Intermittent)
                    .then_some(traffic_direction),
                kind,
                terminal,
                parameters_required: action_parameters_required(kind),
            })
            .collect(),
    }
}

const fn action_parameters_required(kind: RuleActionKind) -> bool {
    use RuleActionKind as Action;
    match kind {
        Action::DisconnectBeforeUpstream => false,
        Action::SetJsonField
        | Action::ReplaceBodyText
        | Action::SetHeader
        | Action::Delay
        | Action::Jitter
        | Action::Throttle
        | Action::Intermittent
        | Action::CustomHttpStatus
        | Action::UpstreamConnectTimeout
        | Action::UpstreamWriteTimeout
        | Action::UpstreamReadTimeout
        | Action::DropUpstreamResponse
        | Action::MockResponse
        | Action::InvalidJson
        | Action::IncorrectContentLength
        | Action::TruncateResponse
        | Action::DisconnectDuringUpstreamWrite
        | Action::DisconnectDuringDownstreamWrite => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_rule_editor_exposes_only_message_match_fields() {
        for stage in [RuleStage::ProxyToUpstream, RuleStage::ProxyToApp] {
            let kinds = stage_capability(stage)
                .match_fields
                .into_iter()
                .map(|field| field.kind)
                .collect::<Vec<_>>();
            assert_eq!(
                kinds,
                vec![
                    RuleMatchFieldKind::Method,
                    RuleMatchFieldKind::RequestTarget,
                ]
            );
        }
    }

    #[test]
    fn http_rule_editor_excludes_redundant_manual_actions() {
        use RuleActionKind as Action;

        let request = stage_capability(RuleStage::ProxyToUpstream)
            .actions
            .into_iter()
            .map(|action| action.kind)
            .collect::<Vec<_>>();
        let response = stage_capability(RuleStage::ProxyToApp)
            .actions
            .into_iter()
            .map(|action| action.kind)
            .collect::<Vec<_>>();

        for actions in [&request, &response] {
            assert!(!actions.contains(&Action::SetJsonField));
            assert!(!actions.contains(&Action::SetHeader));
            assert!(!actions.contains(&Action::MockResponse));
            assert!(actions.contains(&Action::ReplaceBodyText));
        }
    }
}
