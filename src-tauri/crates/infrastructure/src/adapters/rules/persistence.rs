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

pub(super) fn deserialize_persisted_rule(
    mut value: Value,
    legacy_terminal_body_fields: &[&str],
) -> Result<Rule, String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "rule root must be an object".to_owned())?;
    let version = take_rule_persistence_version(object)?;
    let has_legacy_body = has_legacy_terminal_body(object, legacy_terminal_body_fields);

    match (version, has_legacy_body) {
        (Some(RULE_PERSISTENCE_VERSION) | None, false) => {}
        (None, true) => {
            migrate_legacy_terminal_bodies(object, legacy_terminal_body_fields)?;
        }
        (Some(version), _) => {
            return Err(format!("unsupported rule persistence version {version}"));
        }
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
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

pub(super) fn has_legacy_terminal_body(rule: &Map<String, Value>, legacy_fields: &[&str]) -> bool {
    terminal_action_objects(rule).any(|action| {
        legacy_fields
            .iter()
            .any(|field| action.contains_key(*field))
    })
}

pub(super) fn migrate_legacy_terminal_bodies(
    rule: &mut Map<String, Value>,
    legacy_fields: &[&str],
) -> Result<(), String> {
    for action in terminal_action_objects_mut(rule) {
        let mut legacy_body = None;
        for field in legacy_fields {
            let Some(value) = action.remove(*field) else {
                continue;
            };
            if legacy_body.replace(value).is_some() {
                return Err("terminal action contains multiple legacy body fields".into());
            }
        }
        let Some(legacy_body) = legacy_body else {
            continue;
        };
        if action.insert("body_bytes".into(), legacy_body).is_some() {
            return Err("terminal action contains both legacy and current body fields".into());
        }
    }
    Ok(())
}

pub(super) fn terminal_action_objects(
    rule: &Map<String, Value>,
) -> impl Iterator<Item = &Map<String, Value>> {
    rule.get("actions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|action| action.get("Terminal"))
        .filter_map(Value::as_object)
        .filter_map(|terminal| {
            terminal
                .get("MockResponse")
                .or_else(|| terminal.get("InvalidJson"))
        })
        .filter_map(Value::as_object)
}

pub(super) fn terminal_action_objects_mut(
    rule: &mut Map<String, Value>,
) -> impl Iterator<Item = &mut Map<String, Value>> {
    rule.get_mut("actions")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(|action| action.get_mut("Terminal"))
        .filter_map(Value::as_object_mut)
        .filter_map(|terminal| {
            if terminal.contains_key("MockResponse") {
                terminal.get_mut("MockResponse")
            } else {
                terminal.get_mut("InvalidJson")
            }
        })
        .filter_map(Value::as_object_mut)
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
