//! HTTP 规则编辑能力矩阵。
//!
//! 这里是 UI 可选项的 Rust 真相源；领域层保存校验仍是最终防线。两层共同保证前端
//! 不展示不可能的组合，旧客户端或损坏输入也无法绕过业务约束。

use super::Application;
use crate::{
    MessageStage, RuleActionCapabilityViewModel, RuleActionKind, RuleMatchFieldKind,
    RuleStageCapabilityViewModel, RuleTrafficDirection,
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

#[cfg(test)]
pub(super) fn match_field_supported(stage: MessageStage, kind: RuleMatchFieldKind) -> bool {
    stage_capability(stage).match_field_kinds.contains(&kind)
}

pub(super) fn stage_capability(stage: MessageStage) -> RuleStageCapabilityViewModel {
    use RuleActionKind as Action;
    use RuleMatchFieldKind as Field;

    let match_field_kinds = match stage {
        MessageStage::TlsHandshake => vec![Field::CertificateFingerprint],
        MessageStage::Request | MessageStage::Response => vec![
            Field::TerminalIp,
            Field::CertificateFingerprint,
            Field::PathOrRequestType,
            Field::JsonPath,
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
        match_field_kinds,
        actions: kinds
            .into_iter()
            .map(|(kind, terminal)| RuleActionCapabilityViewModel {
                traffic_direction: matches!(kind, Action::Throttle | Action::Intermittent)
                    .then_some(traffic_direction)
                    .flatten(),
                kind,
                terminal,
            })
            .collect(),
    }
}
