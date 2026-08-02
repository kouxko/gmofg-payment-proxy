//! 规则编辑、校验、持久化与故障模板用例。
//!
//! 规则输入解析和修改保护集中在这里，与生命周期、流量和设置流程隔离；所有展示适配器
//! 仍通过稳定的 [`Application`] API 调用。

use std::collections::BTreeMap;

use http::{HeaderName, HeaderValue};

use super::{
    Application,
    validation::{ensure_valid, require_confirmation},
};
use crate::{
    ActiveFaultViewModel, AppError, AppResult, FaultConfigurationDraft, FaultTemplateViewModel,
    OperationResultViewModel, RuleAction, RuleActionKind, RuleByteInputViewModel, RuleCondition,
    RuleConditionKind, RuleDraft, RuleDropResponseMode, RuleHeaderInputViewModel, RuleId,
    RuleJitterScope, RuleMatchField, RuleMatchFieldKind, RuleMatchOperator, RuleMatchOperatorKind,
    RuleSummaryViewModel, RuleTerminalAction, RuleTrafficDirection, RuleViewModel, SessionId,
};

impl Application {
    pub async fn rule_list(&self) -> AppResult<Vec<RuleSummaryViewModel>> {
        let mut rules = self.rules.list().await?;
        rules.sort_by_key(|rule| (rule.priority, rule.creation_order, rule.rule_id));
        Ok(rules)
    }

    pub async fn rule_get(&self, rule_id: RuleId) -> AppResult<RuleViewModel> {
        self.rules.get(rule_id).await
    }

    pub async fn rule_new_draft(&self) -> AppResult<RuleDraft> {
        self.rules.new_draft().await
    }

    pub fn rule_condition_draft(&self, kind: RuleConditionKind) -> RuleCondition {
        match kind {
            RuleConditionKind::Field => RuleCondition::Field {
                field: RuleMatchField::PathOrRequestType,
                operator: RuleMatchOperator::Equals {
                    value: String::new(),
                },
            },
            RuleConditionKind::NthHit => RuleCondition::NthHit { count: 1 },
        }
    }

    pub fn rule_match_field_draft(&self, kind: RuleMatchFieldKind) -> RuleMatchField {
        match kind {
            RuleMatchFieldKind::TerminalIp => RuleMatchField::TerminalIp,
            RuleMatchFieldKind::CertificateFingerprint => RuleMatchField::CertificateFingerprint,
            RuleMatchFieldKind::PathOrRequestType => RuleMatchField::PathOrRequestType,
            RuleMatchFieldKind::JsonPath => RuleMatchField::JsonPath {
                path: "$.field".into(),
            },
        }
    }

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

    pub fn rule_action_draft(&self, kind: RuleActionKind) -> RuleAction {
        match kind {
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
                direction: RuleTrafficDirection::Upstream,
            },
            RuleActionKind::Intermittent => RuleAction::Intermittent {
                available_milliseconds: 1000,
                blocked_milliseconds: 1000,
                direction: RuleTrafficDirection::Upstream,
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
        }
    }

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

    pub async fn rule_create_from_session(&self, session_id: SessionId) -> AppResult<RuleDraft> {
        self.sessions.get(session_id).await?;
        self.rules.create_from_session(session_id).await
    }

    pub async fn rule_save(&self, mut draft: RuleDraft) -> AppResult<RuleViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_rule_or_fault_write_allowed().await?;
        draft.name = draft.name.trim().to_owned();
        draft.description = draft.description.trim().to_owned();
        let validation = self.rules.validate(&draft).await?;
        ensure_valid("RULE_INVALID", "规则配置校验失败。", &validation)?;
        self.rules.save(draft).await
    }

    pub async fn rule_copy(&self, rule_id: RuleId) -> AppResult<RuleViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_rule_or_fault_write_allowed().await?;
        self.rules.copy(rule_id).await
    }

    pub async fn rule_delete(
        &self,
        rule_id: RuleId,
        expected_revision: u64,
        confirmed: bool,
    ) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        require_confirmation(confirmed, "删除规则需要确认。")?;
        self.ensure_rule_or_fault_write_allowed().await?;
        self.rules.delete(rule_id, expected_revision).await
    }

    pub async fn rule_toggle(
        &self,
        rule_id: RuleId,
        expected_revision: u64,
        enabled: bool,
    ) -> AppResult<RuleViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_rule_or_fault_write_allowed().await?;
        self.rules.toggle(rule_id, expected_revision, enabled).await
    }

    pub async fn rule_import(&self) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_rule_or_fault_write_allowed().await?;
        self.rules.import().await
    }

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
        let _gate = self.mutation_gate.lock().await;
        self.ensure_rule_or_fault_write_allowed().await?;
        self.faults.configure(draft).await
    }

    pub async fn fault_active_list(&self) -> AppResult<Vec<ActiveFaultViewModel>> {
        let mut active = self.faults.active().await?;
        active.sort_by_key(|fault| (fault.priority, fault.rule_id));
        Ok(active)
    }

    pub async fn fault_stop(
        &self,
        rule_id: RuleId,
        expected_revision: u64,
        confirmed: bool,
    ) -> AppResult<ActiveFaultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        require_confirmation(confirmed, "停止活动故障需要确认。")?;
        self.ensure_rule_or_fault_write_allowed().await?;
        self.faults.stop(rule_id, expected_revision).await
    }
}
