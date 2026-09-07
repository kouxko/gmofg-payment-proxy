use intercept_proxy_application::AppResult;
use intercept_proxy_domain::{
    Condition, DocumentSchemaNode, RuleDefinition, RuleProgramEntry, UnifiedAction,
    validate_document_condition_schema, validate_unified_action_schema,
};

pub(super) fn compile_rule_entry(
    definition: &RuleDefinition,
    condition: &Condition,
    action: &UnifiedAction,
    schema: Option<&DocumentSchemaNode>,
) -> AppResult<RuleProgramEntry> {
    if let Some(schema) = schema {
        validate_document_condition_schema(condition, schema)?;
        validate_unified_action_schema(action, schema)?;
    }
    Ok(RuleProgramEntry::new(
        definition.rule_id(),
        definition.priority(),
        definition.created_order(),
        condition.clone(),
        action.clone(),
    )?)
}
