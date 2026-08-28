use super::{
    AppError, AppMessageStage, AppResult, AppRuleDraft, AppRuleId, BTreeMap,
    FieldValidationViewModel, ListenerDataPlane, OperationResultViewModel, RULE_IMPORT_MAX_BYTES,
    Rule, RuleEngine, RuleRepositoryAdapter, RuleSummaryViewModel, RuleValidationViewModel,
    RuleViewModel, RuntimeEpoch, Value, cancelled, deserialize_persisted_rule, infra, json_error,
    summary, to_domain_draft, validate_persisted_rule, validate_rule_draft, validation_from_domain,
    view,
};

impl RuleRepositoryAdapter {
    pub(crate) async fn list(&self) -> AppResult<Vec<RuleSummaryViewModel>> {
        let rules = self
            .executor
            .execute(RuleRepositoryAdapter::load_from)
            .await?;
        rules
            .iter()
            .map(|rule| summary(rule, &self.channel_names))
            .collect()
    }

    pub(crate) async fn get(&self, rule_id: AppRuleId) -> AppResult<RuleViewModel> {
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

    pub(crate) fn new_http_draft(channel: intercept_proxy_domain::ChannelId) -> AppRuleDraft {
        AppRuleDraft {
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
        }
    }

    pub(crate) async fn validate(
        &self,
        draft: &AppRuleDraft,
    ) -> AppResult<RuleValidationViewModel> {
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
        let rules = workspace.http_runtime_rules()?;
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

    pub(crate) async fn save(&self, draft: AppRuleDraft) -> AppResult<RuleViewModel> {
        let saved = self
            .executor
            .execute(move |store| RuleRepositoryAdapter::save_locked_to(store, &draft))
            .await?;
        view(&saved, &self.channel_names)
    }

    pub(crate) async fn toggle(
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

    pub(crate) async fn import(&self) -> AppResult<OperationResultViewModel> {
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
                current.replace_http_runtime_rules(
                    RuleEngine::new(RuntimeEpoch::new(), rules).rules().to_vec(),
                )?;
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
}
