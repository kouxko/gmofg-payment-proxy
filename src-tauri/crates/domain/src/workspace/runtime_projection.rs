use super::{
    ChannelId, Condition, DomainError, HttpAction, ProxyWorkspace, Rule, RuleContent,
    RuleDefinition, RuleId, RuleStage, legacy_http_parts, message_stage_from_rule,
    runtime_priority, unified_persistence_error,
};
use crate::HttpRuleContent;

impl ProxyWorkspace {
    pub fn http_runtime_rule_execution_order(&self) -> Vec<RuleId> {
        let mut definitions = self
            .rule_definitions
            .iter()
            .filter(|definition| matches!(definition.content(), RuleContent::Http(_)))
            .collect::<Vec<_>>();
        definitions.sort_by_key(|definition| runtime_order_key(definition));
        definitions
            .into_iter()
            .map(RuleDefinition::rule_id)
            .collect()
    }

    /// Returns the deterministic actor order for the single HTTP + Socket rule collection.
    pub fn runtime_rule_execution_order(&self) -> Vec<RuleId> {
        let mut definitions = self.rule_definitions.iter().collect::<Vec<_>>();
        definitions.sort_by_key(|definition| runtime_order_key(definition));
        definitions
            .into_iter()
            .map(RuleDefinition::rule_id)
            .collect()
    }

    pub fn http_runtime_rules(&self) -> Result<Vec<Rule>, DomainError> {
        let mut rules = Vec::new();
        for definition in &self.rule_definitions {
            let RuleContent::Http(content) = definition.content() else {
                continue;
            };
            let (conditions, actions) = if content.document.is_some() {
                unified_actor_parts(content)
            } else {
                legacy_http_parts(content)?
            };
            if should_skip_empty_http_rule(definition, content, &conditions) {
                continue;
            }
            rules.push(project_runtime_rule(
                definition,
                content.description.clone(),
                conditions,
                actions,
            )?);
        }
        Ok(rules)
    }

    /// Projects the authoritative HTTP + Socket collection into the shared runtime actor model.
    pub fn runtime_rules(&self) -> Result<Vec<Rule>, DomainError> {
        let mut rules = Vec::new();
        for definition in &self.rule_definitions {
            let (description, conditions, actions) = match definition.content() {
                RuleContent::Http(content) => {
                    let (conditions, actions) = if content.document.is_some() {
                        unified_actor_parts(content)
                    } else {
                        legacy_http_parts(content)?
                    };
                    if should_skip_empty_http_rule(definition, content, &conditions) {
                        continue;
                    }
                    (content.description.clone(), conditions, actions)
                }
                RuleContent::Socket(_) => (String::new(), Vec::new(), Vec::new()),
            };
            rules.push(project_runtime_rule(
                definition,
                description,
                conditions,
                actions,
            )?);
        }
        Ok(rules)
    }

    /// Applies lifecycle-only actor output while preserving each rule's authoritative content.
    pub fn replace_runtime_rule_lifecycle(&mut self, rules: Vec<Rule>) -> Result<(), DomainError> {
        let current = self.runtime_rules()?;
        let mut current_ids = current.iter().map(|rule| rule.id).collect::<Vec<_>>();
        let mut next_ids = rules.iter().map(|rule| rule.id).collect::<Vec<_>>();
        current_ids.sort_unstable();
        next_ids.sort_unstable();
        if current_ids != next_ids {
            return Err(unified_persistence_error(
                "rules",
                "运行态生命周期提交不得增加、删除或重复规则",
            ));
        }
        for rule in rules {
            let original = current
                .iter()
                .find(|candidate| candidate.id == rule.id)
                .expect("runtime rule IDs were checked");
            let mut allowed = original.clone();
            allowed.enabled = rule.enabled;
            allowed.revision = rule.revision;
            allowed.hit_count = rule.hit_count;
            allowed.last_hit_at = rule.last_hit_at;
            if allowed != rule {
                return Err(unified_persistence_error(
                    "rules",
                    "运行态生命周期提交包含配置变更",
                ));
            }
            let definition = self
                .rule_definitions
                .iter_mut()
                .find(|definition| definition.rule_id() == rule.id)
                .expect("runtime rule definition exists");
            *definition = RuleDefinition::restore(
                definition.rule_id(),
                {
                    let mut draft = definition.to_draft();
                    draft.enabled = rule.enabled;
                    draft
                },
                crate::RuleDefinitionRestoreSnapshot {
                    revision: rule.revision,
                    created_order: definition.created_order(),
                    lifecycle: crate::RuleLifecycle {
                        hit_count: rule.hit_count,
                        last_hit_at: rule.last_hit_at,
                    },
                },
            )?;
        }
        Ok(())
    }

    /// Clears transient hit metadata for every HTTP and Socket rule in the collection.
    pub fn reset_runtime_rule_hit_metadata(&mut self) -> Result<bool, DomainError> {
        let mut changed = false;
        for definition in &mut self.rule_definitions {
            if definition.lifecycle().hit_count == 0 && definition.lifecycle().last_hit_at.is_none()
            {
                continue;
            }
            changed = true;
            *definition = RuleDefinition::restore(
                definition.rule_id(),
                definition.to_draft(),
                crate::RuleDefinitionRestoreSnapshot {
                    revision: definition.revision(),
                    created_order: definition.created_order(),
                    lifecycle: crate::RuleLifecycle::default(),
                },
            )?;
        }
        Ok(changed)
    }
}

fn unified_actor_parts(content: &HttpRuleContent) -> (Vec<Condition>, Vec<HttpAction>) {
    let actions = content
        .actions
        .iter()
        .filter_map(|action| match action {
            crate::UnifiedAction::Http(action) => Some(action.clone()),
            crate::UnifiedAction::Terminal(action) => Some(HttpAction::Terminal(action.clone())),
            crate::UnifiedAction::RecordMatch | crate::UnifiedAction::Document(_) => None,
        })
        .collect();
    (Vec::new(), actions)
}

fn runtime_order_key(definition: &RuleDefinition) -> (u8, u8, i32, RuleId) {
    let (direction, phase) = match definition.stage() {
        RuleStage::ProxyToUpstream => (0, 0),
        RuleStage::ProxyToApp => (1, 0),
        RuleStage::TlsHandshake => (2, 0),
    };
    (
        direction,
        phase,
        definition.priority(),
        definition.rule_id(),
    )
}

fn should_skip_empty_http_rule(
    definition: &RuleDefinition,
    content: &HttpRuleContent,
    conditions: &[Condition],
) -> bool {
    conditions.is_empty()
        && content.actions.is_empty()
        && content.document.is_none()
        && content.description.is_empty()
        && !definition.one_shot()
        && definition.lifecycle().hit_count == 0
        && definition.lifecycle().last_hit_at.is_none()
}

fn project_runtime_rule(
    definition: &RuleDefinition,
    description: String,
    conditions: Vec<Condition>,
    actions: Vec<HttpAction>,
) -> Result<Rule, DomainError> {
    Ok(Rule {
        id: definition.rule_id(),
        revision: definition.revision(),
        name: definition.name().to_owned(),
        description,
        enabled: definition.enabled(),
        priority: runtime_priority(definition.priority())?,
        created_order: definition.created_order(),
        channel: Some(ChannelId::new(definition.listener_id().to_string())?),
        stage: message_stage_from_rule(definition.stage()),
        conditions,
        actions,
        one_shot: definition.one_shot(),
        hit_count: definition.lifecycle().hit_count,
        last_hit_at: definition.lifecycle().last_hit_at,
    })
}
