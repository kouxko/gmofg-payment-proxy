//! HTTP 规则编辑能力矩阵。
//!
//! 这里是 UI 可选项的 Rust 真相源；领域层保存校验仍是最终防线。两层共同保证前端
//! 不展示不可能的组合，旧客户端或损坏输入也无法绕过业务约束。

use super::Application;
use crate::{
    MessageStage, RuleActionCapabilityViewModel, RuleActionKind, RuleMatchFieldCapabilityViewModel,
    RuleMatchFieldKind, RuleMatchOperatorKind, RuleMatchSelectorKind, RuleStageCapabilityViewModel,
    RuleTrafficDirection,
};

impl Application {
    #[must_use]
    pub fn rule_capabilities(&self) -> Vec<RuleStageCapabilityViewModel> {
        [
            MessageStage::TlsHandshake,
            MessageStage::Request,
            MessageStage::Response,
        ]
        .into_iter()
        .map(stage_capability)
        .collect()
    }
}

pub(super) fn action_capability(
    stage: MessageStage,
    kind: RuleActionKind,
) -> Option<RuleActionCapabilityViewModel> {
    stage_capability(stage)
        .actions
        .into_iter()
        .find(|capability| capability.kind == kind)
}

pub(super) fn stage_capability(stage: MessageStage) -> RuleStageCapabilityViewModel {
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
    let match_fields = match stage {
        MessageStage::TlsHandshake => vec![capability(
            Field::CertificateFingerprint,
            string_operators.clone(),
            None,
        )],
        MessageStage::Request | MessageStage::Response => vec![
            capability(Field::TerminalIp, string_operators.clone(), None),
            capability(
                Field::CertificateFingerprint,
                string_operators.clone(),
                None,
            ),
            capability(Field::Method, vec![Operator::Equals], None),
            capability(Field::RequestTarget, string_operators.clone(), None),
            capability(
                Field::Header,
                string_operators,
                Some(RuleMatchSelectorKind::HeaderNamePointer),
            ),
        ],
        MessageStage::Terminal => Vec::new(),
    };
    let common = [
        Action::SetJsonField,
        Action::ReplaceBodyText,
        Action::SetHeader,
        Action::Delay,
        Action::Jitter,
        Action::Throttle,
        Action::Intermittent,
        Action::Pause,
    ];
    let kinds: Vec<(RuleActionKind, bool)> = match stage {
        MessageStage::TlsHandshake => vec![(Action::RejectTlsHandshake, true)],
        MessageStage::Request => common
            .into_iter()
            .map(|kind| (kind, false))
            .chain([
                (Action::DisconnectBeforeUpstream, true),
                (Action::UpstreamConnectTimeout, true),
                (Action::UpstreamWriteTimeout, true),
                (Action::UpstreamReadTimeout, true),
                (Action::DropUpstreamResponse, true),
                (Action::MockResponse, true),
                (Action::DisconnectDuringUpstreamWrite, true),
            ])
            .collect(),
        MessageStage::Response => common
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
        MessageStage::Terminal => Vec::new(),
    };
    let traffic_direction = match stage {
        MessageStage::Request => Some(RuleTrafficDirection::Upstream),
        MessageStage::Response => Some(RuleTrafficDirection::Downstream),
        MessageStage::TlsHandshake | MessageStage::Terminal => None,
    };
    RuleStageCapabilityViewModel {
        stage,
        match_fields,
        actions: kinds
            .into_iter()
            .map(|(kind, terminal)| RuleActionCapabilityViewModel {
                traffic_direction: matches!(kind, Action::Throttle | Action::Intermittent)
                    .then_some(traffic_direction)
                    .flatten(),
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
        Action::Pause | Action::RejectTlsHandshake | Action::DisconnectBeforeUpstream => false,
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
