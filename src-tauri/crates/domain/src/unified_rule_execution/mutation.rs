use crate::{Document, DocumentSchemaNode, DomainError, HttpAction};

use super::{DocumentMutation, UnifiedAction, rule_error};

impl DocumentMutation {
    pub fn apply(&self, document: &mut Document) -> Result<(), DomainError> {
        match self {
            Self::Set { path, value } => document.set(path, value.clone()),
            Self::Clear { path, .. } => document.clear_path(path),
            Self::Insert { path, index, value } => document.insert(path, *index, value.clone()),
            Self::Append { path, value } => document.append(path, value.clone()),
        }
    }
}

/// Validates Document mutation values at paths declared by the package Schema.
pub fn validate_unified_actions_schema(
    actions: &[UnifiedAction],
    schema: &DocumentSchemaNode,
) -> Result<(), DomainError> {
    for (index, action) in actions.iter().enumerate() {
        let expected_and_type = match action {
            UnifiedAction::Document(DocumentMutation::Set { path, value }) => schema
                .resolve(path)
                .ok()
                .map(|node| (node, value.value_type())),
            UnifiedAction::Document(DocumentMutation::Clear { path, value_type }) => {
                schema.resolve(path).ok().map(|node| (node, *value_type))
            }
            UnifiedAction::Document(
                DocumentMutation::Insert { path, value, .. }
                | DocumentMutation::Append { path, value },
            ) => match schema.resolve(path).ok() {
                Some(DocumentSchemaNode::Array { items, .. }) => {
                    Some((items.as_ref(), value.value_type()))
                }
                Some(_) => {
                    return Err(rule_error(
                        &format!("actions.{index}"),
                        "动作值类型与 Schema 声明不一致",
                    ));
                }
                None => None,
            },
            UnifiedAction::RecordMatch | UnifiedAction::Http(_) | UnifiedAction::Terminal(_) => {
                None
            }
        };
        if let Some((expected, value_type)) = expected_and_type
            && !expected.accepts(value_type)
        {
            return Err(rule_error(
                &format!("actions.{index}"),
                "动作值类型与 Schema 声明不一致",
            ));
        }
    }
    Ok(())
}

impl From<HttpAction> for UnifiedAction {
    fn from(action: HttpAction) -> Self {
        match action {
            HttpAction::Terminal(action) => Self::Terminal(action),
            action => Self::Http(action),
        }
    }
}
