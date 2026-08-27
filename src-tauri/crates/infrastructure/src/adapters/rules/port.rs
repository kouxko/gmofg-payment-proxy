use super::{
    AppError, AppMessageStage, AppResult, AppRuleDraft, AppRuleId, BTreeMap,
    FieldValidationViewModel, ListenerDataPlane, MatchCondition, OperationResultViewModel,
    RULE_IMPORT_MAX_BYTES, Rule, RuleEngine, RuleRepositoryAdapter, RuleRepositoryPort,
    RuleSummaryViewModel, RuleValidationViewModel, RuleViewModel, RuntimeEpoch, SessionId, Value,
    app_draft, async_trait, cancelled, condition_to_app, deserialize_persisted_rule, infra,
    json_error, serialize_persisted_rule, summary, to_domain_draft, validate_persisted_rule,
    validate_rule_draft, validation_from_domain, view,
};

#[async_trait]
impl RuleRepositoryPort for RuleRepositoryAdapter {
    async fn list(&self) -> AppResult<Vec<RuleSummaryViewModel>> {
        let rules = self
            .executor
            .execute(RuleRepositoryAdapter::load_from)
            .await?;
        rules
            .iter()
            .map(|rule| summary(rule, &self.channel_names))
            .collect()
    }

    async fn get(&self, rule_id: AppRuleId) -> AppResult<RuleViewModel> {
        let rule = self
            .executor
            .execute(move |store| {
                RuleRepositoryAdapter::load_from(store)?
                    .into_iter()
                    .find(|rule| rule.id.as_uuid() == rule_id)
                    .ok_or_else(|| {
                        AppError::new("RULE_INVALID", "规则不存在。").entity(rule_id.to_string())
                    })
            })
            .await?;
        view(&rule, &self.channel_names)
    }

    async fn new_http_draft(
        &self,
        channel: intercept_proxy_domain::ChannelId,
    ) -> AppResult<AppRuleDraft> {
        Ok(AppRuleDraft {
            rule_id: None,
            expected_revision: None,
            name: "新建规则".into(),
            description: String::new(),
            enabled: true,
            priority: 100,
            channel: Some(channel),
            stage: Some(AppMessageStage::Request),
            conditions: Vec::new(),
            actions: Vec::new(),
            one_shot: false,
        })
    }

    async fn create_from_session(&self, session_id: SessionId) -> AppResult<AppRuleDraft> {
        let session = self.sessions.get(session_id).await?;
        let condition = MatchCondition::Field {
            field: intercept_proxy_domain::MatchField::PathOrRequestType,
            operator: intercept_proxy_domain::MatchOperator::Equals(session.summary.target.clone()),
        };
        Ok(AppRuleDraft {
            rule_id: None,
            expected_revision: None,
            name: format!("匹配 {}", session.summary.target),
            description: format!(
                "基于请求 {} 创建，请确认动作后保存。",
                session.summary.request_id
            ),
            enabled: true,
            priority: 100,
            channel: Some(session.summary.channel),
            stage: Some(AppMessageStage::Request),
            conditions: vec![condition_to_app(&condition)],
            actions: Vec::new(),
            one_shot: false,
        })
    }

    async fn validate(&self, draft: &AppRuleDraft) -> AppResult<RuleValidationViewModel> {
        let workspace = self
            .executor
            .execute(RuleRepositoryAdapter::load_selected_workspace_from)
            .await?;
        let binding_error = match &draft.channel {
            None => Some("普通 HTTP 规则必须绑定单个 HTTP 代理入口"),
            Some(channel) => workspace
                .listeners
                .iter()
                .find(|listener| listener.id.to_string() == channel.as_str())
                .map_or(
                    Some("规则通道必须引用当前 Workspace 中存在的代理入口"),
                    |listener| {
                        (!matches!(listener.data_plane, ListenerDataPlane::Http(_)))
                            .then_some("普通 HTTP 规则只能绑定 HTTP 代理入口")
                    },
                ),
        };
        if let Some(message) = binding_error {
            return Ok(FieldValidationViewModel {
                valid: false,
                field_errors: BTreeMap::from([("channel".into(), vec![message.into()])]),
                warnings: Vec::new(),
            });
        }
        let rules = workspace.rules;
        let creation_order = rules
            .iter()
            .map(|rule| rule.created_order)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        match to_domain_draft(draft, creation_order).and_then(|draft| {
            validate_rule_draft(&draft)?;
            Ok(draft)
        }) {
            Ok(candidate) => {
                let mut warnings = Vec::new();
                if let Ok(rule) = Rule::create(candidate) {
                    let mut all = rules;
                    all.push(rule);
                    warnings.extend(
                        RuleEngine::new(RuntimeEpoch::new(), all)
                            .conflict_warnings()
                            .into_iter()
                            .map(|warning| warning.message),
                    );
                }
                Ok(FieldValidationViewModel {
                    valid: true,
                    field_errors: BTreeMap::default(),
                    warnings,
                })
            }
            Err(error) => Ok(validation_from_domain(&error)),
        }
    }

    async fn save(&self, draft: AppRuleDraft) -> AppResult<RuleViewModel> {
        let saved = self
            .executor
            .execute(move |store| RuleRepositoryAdapter::save_locked_to(store, &draft))
            .await?;
        view(&saved, &self.channel_names)
    }

    async fn copy(&self, rule_id: AppRuleId) -> AppResult<RuleViewModel> {
        let saved = self
            .executor
            .execute(move |store| {
                let source = RuleRepositoryAdapter::load_from(store)?
                    .into_iter()
                    .find(|rule| rule.id.as_uuid() == rule_id)
                    .ok_or_else(|| {
                        AppError::new("RULE_INVALID", "规则不存在。").entity(rule_id.to_string())
                    })?;
                let mut draft = app_draft(&source)?;
                draft.rule_id = None;
                draft.expected_revision = None;
                draft.name = format!("{}（副本）", draft.name);
                RuleRepositoryAdapter::save_locked_to(store, &draft)
            })
            .await?;
        view(&saved, &self.channel_names)
    }

    async fn delete(
        &self,
        rule_id: AppRuleId,
        expected_revision: u64,
    ) -> AppResult<OperationResultViewModel> {
        self.executor
            .execute(move |store| {
                let mut workspace = RuleRepositoryAdapter::load_selected_workspace_from(store)?;
                let rule = workspace
                    .rules
                    .iter()
                    .find(|rule| rule.id.as_uuid() == rule_id)
                    .ok_or_else(|| AppError::new("RULE_INVALID", "规则不存在。"))?;
                if rule.revision.get() != expected_revision {
                    return Err(AppError::new("REVISION_CONFLICT", "规则已被其他操作更新。"));
                }
                let expected_workspace_revision = workspace.revision.get();
                workspace.rules.retain(|rule| rule.id.as_uuid() != rule_id);
                RuleRepositoryAdapter::save_selected_workspace_to(
                    store,
                    workspace,
                    expected_workspace_revision,
                )?;
                Ok(())
            })
            .await?;
        Ok(OperationResultViewModel::success("规则已删除。"))
    }

    async fn toggle(
        &self,
        rule_id: AppRuleId,
        expected_revision: u64,
        enabled: bool,
    ) -> AppResult<RuleViewModel> {
        let changed = self
            .executor
            .execute(move |store| {
                RuleRepositoryAdapter::toggle_domain_to(store, rule_id, expected_revision, enabled)
            })
            .await?;
        view(&changed, &self.channel_names)
    }

    async fn import(&self) -> AppResult<OperationResultViewModel> {
        let selected = self
            .executor
            .execute(RuleRepositoryAdapter::load_selected_workspace_from)
            .await?;
        let expected_workspace_id = selected.id;
        let expected_workspace_revision = selected.revision.get();
        let Some(path) = self.dialog.choose_open_file("rules_json")? else {
            return Ok(cancelled("已取消规则导入。"));
        };
        let bytes = infra(self.exporter.read_bounded(&path, RULE_IMPORT_MAX_BYTES))?;
        let values: Vec<Value> = serde_json::from_slice(&bytes)
            .map_err(|error| json_error("规则导入文件无效", error))?;
        let rules = values
            .into_iter()
            .map(deserialize_persisted_rule)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| json_error("规则导入文件无效", error))?;
        for rule in &rules {
            validate_persisted_rule(rule).map_err(AppError::from)?;
        }
        let imported_count = rules.len();
        self.executor
            .execute(move |store| {
                let mut current = RuleRepositoryAdapter::load_selected_workspace_from(store)?;
                if current.id != expected_workspace_id
                    || current.revision.get() != expected_workspace_revision
                {
                    return Err(AppError::new(
                        "REVISION_CONFLICT",
                        "导入期间当前 Workspace 已切换或被更新。",
                    ));
                }
                current.rules = RuleEngine::new(RuntimeEpoch::new(), rules).rules().to_vec();
                RuleRepositoryAdapter::save_selected_workspace_to(
                    store,
                    current,
                    expected_workspace_revision,
                )?;
                Ok(())
            })
            .await?;
        Ok(OperationResultViewModel::success(format!(
            "已导入 {imported_count} 条规则。"
        )))
    }

    async fn export(&self) -> AppResult<OperationResultViewModel> {
        let Some(selection) = self.dialog.choose_save_file("rules_json", "rules.json")? else {
            return Ok(cancelled("已取消规则导出。"));
        };
        let rules = self
            .executor
            .execute(RuleRepositoryAdapter::load_from)
            .await?
            .iter()
            .map(serialize_persisted_rule)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| json_error("规则导出序列化失败", error))?;
        let bytes = serde_json::to_vec_pretty(&rules)
            .map_err(|error| json_error("规则导出序列化失败", error))?;
        infra(
            self.exporter
                .write(&selection.path, &bytes, selection.overwrite_confirmed),
        )?;
        Ok(OperationResultViewModel::success("规则已导出。"))
    }
}
