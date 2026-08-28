//! 规则编辑、校验、持久化与故障模板用例。
//!
//! 规则输入解析和修改保护集中在这里，与生命周期、流量和设置流程隔离；所有展示适配器
//! 仍通过稳定的 [`Application`] API 调用。

#[cfg(test)]
use std::collections::BTreeMap;

mod exchange_mock;

#[cfg(test)]
use http::{HeaderName, HeaderValue};

#[cfg(test)]
use super::rule_capabilities::match_field_supported;
#[cfg(test)]
use super::validation::ensure_valid;
use super::{Application, rule_capabilities::action_capability, validation::require_confirmation};
use crate::{
    ActiveFaultViewModel, AppError, AppResult, FaultConfigurationDraft, FaultTemplateViewModel,
    MessageStage, RuleAction, RuleActionKind, RuleCondition, RuleConditionKind,
    RuleDropResponseMode, RuleId, RuleJitterScope, RuleMatchField, RuleMatchOperator,
    RuleTerminalAction,
};
#[cfg(test)]
use crate::{
    ListenerDataPlane, ListenerId, OperationResultViewModel, RuleByteInputViewModel, RuleDraft,
    RuleHeaderInputViewModel, RuleMatchFieldKind, RuleMatchOperatorKind, RuleSummaryViewModel,
    RuleViewModel, SessionId,
};

impl Application {
    #[cfg(test)]
    pub async fn rule_list(&self) -> AppResult<Vec<RuleSummaryViewModel>> {
        let mut rules = self.rules.list().await?;
        rules.sort_by_key(|rule| (rule.priority, rule.creation_order, rule.rule_id));
        Ok(rules)
    }

    #[cfg(test)]
    pub async fn rule_get(&self, rule_id: RuleId) -> AppResult<RuleViewModel> {
        self.rules.get(rule_id).await
    }

    #[cfg(test)]
    pub async fn rule_new_http_draft(&self, listener_id: ListenerId) -> AppResult<RuleDraft> {
        let selected = self
            .workspaces
            .list()
            .await?
            .into_iter()
            .find(|workspace| workspace.selected)
            .ok_or_else(|| AppError::new("LISTENER_REQUIRED", "当前没有选中的 Workspace。"))?;
        let workspace = self.workspaces.get(selected.id).await?;
        let listener = workspace
            .listeners
            .into_iter()
            .find(|listener| listener.id == listener_id)
            .ok_or_else(|| {
                AppError::new("LISTENER_REQUIRED", "所选 HTTP 入口不存在，请刷新后重试。")
                    .entity(listener_id.to_string())
            })?;
        if !matches!(listener.data_plane, ListenerDataPlane::Http(_)) {
            return Err(AppError::new(
                "LISTENER_INCOMPATIBLE",
                "普通 HTTP 规则只能绑定 HTTP 代理入口。",
            )
            .entity(listener_id.to_string()));
        }
        self.rules
            .new_http_draft(crate::ChannelId::new(listener_id.to_string()).map_err(AppError::from)?)
            .await
    }

    pub(crate) fn rule_condition_draft(
        &self,
        kind: RuleConditionKind,
        stage: MessageStage,
    ) -> RuleCondition {
        let _ = self;
        match kind {
            RuleConditionKind::Field => RuleCondition::Field {
                field: if stage == MessageStage::TlsHandshake {
                    RuleMatchField::CertificateFingerprint
                } else {
                    RuleMatchField::PathOrRequestType
                },
                operator: RuleMatchOperator::Equals {
                    value: String::new(),
                },
            },
            RuleConditionKind::NthHit => RuleCondition::NthHit { count: 1 },
        }
    }

    #[cfg(test)]
    pub fn rule_match_field_draft(
        &self,
        kind: RuleMatchFieldKind,
        stage: MessageStage,
    ) -> AppResult<RuleMatchField> {
        if !match_field_supported(stage, kind) {
            return Err(AppError::new(
                "RULE_INVALID",
                "匹配字段与当前规则阶段不兼容。",
            ));
        }
        Ok(match kind {
            RuleMatchFieldKind::TerminalIp => RuleMatchField::TerminalIp,
            RuleMatchFieldKind::CertificateFingerprint => RuleMatchField::CertificateFingerprint,
            RuleMatchFieldKind::PathOrRequestType => RuleMatchField::PathOrRequestType,
            RuleMatchFieldKind::JsonPath => RuleMatchField::JsonPath {
                path: "$.field".into(),
            },
        })
    }

    #[cfg(test)]
    pub fn rule_match_operator_draft(&self, kind: RuleMatchOperatorKind) -> RuleMatchOperator {
        match kind {
            RuleMatchOperatorKind::Equals => RuleMatchOperator::Equals {
                value: String::new(),
            },
            RuleMatchOperatorKind::Contains => RuleMatchOperator::Contains {
                value: String::new(),
            },
            RuleMatchOperatorKind::Regex => RuleMatchOperator::Regex {
                pattern: String::new(),
            },
        }
    }

    pub(crate) fn rule_action_draft(
        &self,
        kind: RuleActionKind,
        stage: MessageStage,
    ) -> AppResult<RuleAction> {
        let _ = self;
        let capability = action_capability(stage, kind)
            .ok_or_else(|| AppError::new("RULE_INVALID", "动作与当前规则阶段不兼容。"))?;
        let traffic_direction = capability.traffic_direction;
        Ok(match kind {
            RuleActionKind::SetJsonField => RuleAction::SetJsonField {
                path: "$.field".into(),
                value_json: "null".into(),
            },
            RuleActionKind::ReplaceBodyText => RuleAction::ReplaceBodyText {
                text: String::new(),
            },
            RuleActionKind::SetHeader => RuleAction::SetHeader {
                name: "x-proxy-test".into(),
                value: String::new(),
            },
            RuleActionKind::Delay => RuleAction::Delay { milliseconds: 100 },
            RuleActionKind::Jitter => RuleAction::Jitter {
                minimum_milliseconds: 0,
                maximum_milliseconds: 100,
                scope: RuleJitterScope::PerChunk,
            },
            RuleActionKind::Throttle => RuleAction::Throttle {
                bytes_per_second: 1024,
                chunk_bytes: 16 * 1024,
                direction: traffic_direction.expect("throttle capability has a direction"),
            },
            RuleActionKind::Intermittent => RuleAction::Intermittent {
                available_milliseconds: 1000,
                blocked_milliseconds: 1000,
                direction: traffic_direction.expect("intermittent capability has a direction"),
            },
            RuleActionKind::Pause => RuleAction::Pause,
            RuleActionKind::CustomHttpStatus => RuleAction::CustomHttpStatus { status: 500 },
            RuleActionKind::RejectTlsHandshake => RuleAction::Terminal {
                action: RuleTerminalAction::RejectTlsHandshake,
            },
            RuleActionKind::DisconnectBeforeUpstream => RuleAction::Terminal {
                action: RuleTerminalAction::DisconnectBeforeUpstream,
            },
            RuleActionKind::UpstreamConnectTimeout => RuleAction::Terminal {
                action: RuleTerminalAction::UpstreamConnectTimeout {
                    milliseconds: 1_000,
                },
            },
            RuleActionKind::UpstreamWriteTimeout => RuleAction::Terminal {
                action: RuleTerminalAction::UpstreamWriteTimeout {
                    milliseconds: 1_000,
                },
            },
            RuleActionKind::UpstreamReadTimeout => RuleAction::Terminal {
                action: RuleTerminalAction::UpstreamReadTimeout {
                    milliseconds: 1_000,
                },
            },
            RuleActionKind::DropUpstreamResponse => RuleAction::Terminal {
                action: RuleTerminalAction::DropUpstreamResponse {
                    mode: RuleDropResponseMode::ReadCompleteResponse,
                },
            },
            RuleActionKind::MockResponse => RuleAction::Terminal {
                action: RuleTerminalAction::MockResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "application/json".into())],
                    body_bytes: b"{}".to_vec(),
                },
            },
            RuleActionKind::InvalidJson => RuleAction::Terminal {
                action: RuleTerminalAction::InvalidJson {
                    body_bytes: b"{".to_vec(),
                },
            },
            RuleActionKind::IncorrectContentLength => RuleAction::Terminal {
                action: RuleTerminalAction::IncorrectContentLength { delta: 1 },
            },
            RuleActionKind::TruncateResponse => RuleAction::Terminal {
                action: RuleTerminalAction::TruncateResponse { bytes: 1 },
            },
            RuleActionKind::DisconnectDuringUpstreamWrite => RuleAction::Terminal {
                action: RuleTerminalAction::DisconnectDuringUpstreamWrite { after_bytes: 1 },
            },
            RuleActionKind::DisconnectDuringDownstreamWrite => RuleAction::Terminal {
                action: RuleTerminalAction::DisconnectDuringDownstreamWrite { after_bytes: 1 },
            },
        })
    }

    #[cfg(test)]
    pub fn rule_parse_byte_input(&self, raw: &str) -> AppResult<RuleByteInputViewModel> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(RuleByteInputViewModel {
                bytes: Vec::new(),
                normalized: String::new(),
            });
        }
        let mut bytes = Vec::new();
        for (index, part) in trimmed.split(',').enumerate() {
            let value = part.trim().parse::<u8>().map_err(|_| {
                AppError::field(
                    "RULE_INVALID",
                    "字节输入必须是以逗号分隔的 0 到 255 十进制整数。",
                    BTreeMap::from([(
                        "raw".into(),
                        vec![format!(
                            "第 {} 项“{}”不是有效字节。",
                            index + 1,
                            part.trim()
                        )],
                    )]),
                )
            })?;
            bytes.push(value);
        }
        Ok(RuleByteInputViewModel {
            normalized: bytes
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            bytes,
        })
    }

    #[cfg(test)]
    pub fn rule_parse_header_input(&self, raw: &str) -> AppResult<RuleHeaderInputViewModel> {
        let mut headers = Vec::new();
        for (index, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((raw_name, raw_value)) = line.split_once(':') else {
                return Err(AppError::field(
                    "RULE_INVALID",
                    "响应 Header 必须使用每行“name: value”的格式。",
                    BTreeMap::from([(
                        "raw".into(),
                        vec![format!("第 {} 行缺少冒号分隔符。", index + 1)],
                    )]),
                ));
            };
            let name = raw_name.trim();
            let value = raw_value.trim();
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                AppError::field(
                    "RULE_INVALID",
                    "响应 Header 名称无效。",
                    BTreeMap::from([(
                        "raw".into(),
                        vec![format!("第 {} 行包含无效 Header 名称。", index + 1)],
                    )]),
                )
            })?;
            HeaderValue::from_bytes(value.as_bytes()).map_err(|_| {
                AppError::field(
                    "RULE_INVALID",
                    "响应 Header 值无效。",
                    BTreeMap::from([(
                        "raw".into(),
                        vec![format!("第 {} 行包含无效 Header 值。", index + 1)],
                    )]),
                )
            })?;
            headers.push((name.to_ascii_lowercase(), value.to_owned()));
        }
        let normalized = headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(RuleHeaderInputViewModel {
            headers,
            normalized,
        })
    }

    #[cfg(test)]
    pub async fn rule_create_from_session(&self, session_id: SessionId) -> AppResult<RuleDraft> {
        self.sessions.get(session_id).await?;
        self.rules.create_from_session(session_id).await
    }

    #[cfg(test)]
    pub async fn rule_save(&self, mut draft: RuleDraft) -> AppResult<RuleViewModel> {
        let _gate = self.mutation_gate.lock().await;
        draft.name = draft.name.trim().to_owned();
        draft.description = draft.description.trim().to_owned();
        let validation = self.rules.validate(&draft).await?;
        ensure_valid("RULE_INVALID", "规则配置校验失败。", &validation)?;
        self.rules.save(draft).await
    }

    #[cfg(test)]
    pub async fn rule_copy(&self, rule_id: RuleId) -> AppResult<RuleViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.rules.copy(rule_id).await
    }

    #[cfg(test)]
    pub async fn rule_delete(
        &self,
        rule_id: RuleId,
        expected_revision: u64,
        confirmed: bool,
    ) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        require_confirmation(confirmed, "删除规则需要确认。")?;
        self.rules.delete(rule_id, expected_revision).await
    }

    #[cfg(test)]
    pub async fn rule_toggle(
        &self,
        rule_id: RuleId,
        expected_revision: u64,
        enabled: bool,
    ) -> AppResult<RuleViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.rules.toggle(rule_id, expected_revision, enabled).await
    }

    #[cfg(test)]
    pub async fn rule_import(&self) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.rules.import().await
    }

    #[cfg(test)]
    pub async fn rule_export(&self) -> AppResult<OperationResultViewModel> {
        self.rules.export().await
    }

    pub async fn fault_template_list(&self) -> AppResult<Vec<FaultTemplateViewModel>> {
        let mut templates = self.faults.templates().await?;
        let channel = self
            .selected_workspace_channel_catalog()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| {
                AppError::new(
                    "LISTENER_REQUIRED",
                    "当前 Workspace 没有代理入口；请先新增入口再配置故障模拟。",
                )
            })?
            .id;
        for template in &mut templates {
            template.default_channel = channel.clone();
        }
        templates.sort_by(|left, right| left.template_id.cmp(&right.template_id));
        Ok(templates)
    }

    pub async fn fault_configure(
        &self,
        draft: FaultConfigurationDraft,
    ) -> AppResult<ActiveFaultViewModel> {
        let input = self.faults.rule_draft(draft).await?;
        let saved = self.rule_definition_save(input).await?;
        self.faults.active_view(&saved).ok_or_else(|| {
            AppError::new("RULE_INVALID", "故障模板生成的统一规则无法投影为活动故障。")
        })
    }

    pub async fn fault_active_list(&self) -> AppResult<Vec<ActiveFaultViewModel>> {
        let mut active = self
            .rule_definition_list()
            .await?
            .iter()
            .filter_map(|rule| self.faults.active_view(rule))
            .collect::<Vec<_>>();
        active.sort_by_key(|fault| (fault.priority, fault.rule_id));
        Ok(active)
    }

    pub async fn fault_stop(
        &self,
        rule_id: RuleId,
        expected_revision: u64,
        confirmed: bool,
    ) -> AppResult<ActiveFaultViewModel> {
        require_confirmation(confirmed, "停止活动故障需要确认。")?;
        let saved = self
            .rule_definition_toggle(
                intercept_proxy_domain::RuleId::from_uuid(rule_id),
                intercept_proxy_domain::Revision::new(expected_revision),
                false,
            )
            .await?;
        self.faults
            .active_view(&saved)
            .ok_or_else(|| AppError::new("RULE_NOT_FOUND", "指定规则不是活动故障规则。"))
    }
}
