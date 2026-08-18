use super::{
    AppError, InfrastructureError, Map, PERSISTENCE_VERSION_FIELD, RULE_PERSISTENCE_VERSION, Rule,
    RuleDraft, Value, app_error, validate_rule_draft,
};

pub(super) fn persisted_rule_error(message: String) -> AppError {
    app_error(InfrastructureError::PersistenceCorrupt {
        entity: "rule",
        message,
    })
}

pub(super) fn serialize_persisted_rule(rule: &Rule) -> Result<Value, serde_json::Error> {
    let mut value = serde_json::to_value(rule)?;
    value
        .as_object_mut()
        .expect("Rule always serializes as an object")
        .insert(
            PERSISTENCE_VERSION_FIELD.into(),
            Value::from(RULE_PERSISTENCE_VERSION),
        );
    Ok(value)
}

pub(super) fn deserialize_persisted_rule(mut value: Value) -> Result<Rule, String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "rule root must be an object".to_owned())?;
    match take_rule_persistence_version(object)? {
        Some(RULE_PERSISTENCE_VERSION) => {}
        Some(version) => {
            return Err(format!("unsupported rule persistence version {version}"));
        }
        None => return Err("rule persistence version is required".into()),
    }
    let rule: Rule = serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
    let current = serde_json::to_value(&rule).map_err(|error| error.to_string())?;
    if current != value {
        return Err("rule contains unknown or non-current fields".into());
    }
    Ok(rule)
}

pub(super) fn take_rule_persistence_version(
    object: &mut Map<String, Value>,
) -> Result<Option<u64>, String> {
    object
        .remove(PERSISTENCE_VERSION_FIELD)
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| "rule persistence version must be an unsigned integer".to_owned())
        })
        .transpose()
}

pub(super) fn validate_persisted_rule(
    rule: &Rule,
) -> Result<(), intercept_proxy_domain::DomainError> {
    validate_rule_draft(&RuleDraft {
        expected_revision: Some(rule.revision),
        name: rule.name.clone(),
        description: rule.description.clone(),
        enabled: rule.enabled,
        priority: rule.priority,
        created_order: rule.created_order,
        channel: rule.channel.clone(),
        stage: rule.stage,
        conditions: rule.conditions.clone(),
        actions: rule.actions.clone(),
        one_shot: rule.one_shot,
    })
}
