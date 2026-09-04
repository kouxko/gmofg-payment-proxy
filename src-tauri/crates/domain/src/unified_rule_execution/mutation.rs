use crate::{Document, DocumentSchemaNode, DomainError, HttpAction};

use super::{DocumentMutation, UnifiedAction, rule_error};

impl DocumentMutation {
    pub fn apply(&self, document: &mut Document) -> Result<(), DomainError> {
        let path = match self {
            Self::Set { path, .. }
            | Self::Clear { path, .. }
            | Self::Insert { path, .. }
            | Self::Append { path, .. } => path,
        };
        if let Some(pointer) = path.exact_pointer() {
            return self.apply_at(document, &pointer);
        }

        let mut pointers = document.resolve_match_pointers(path);
        if matches!(self, Self::Clear { .. }) {
            // Snapshot traversal yields array indices in ascending order. Reverse application
            // prevents earlier removals from shifting the remaining concrete targets.
            pointers.reverse();
        }
        let mut working = document.clone();
        for pointer in pointers {
            self.apply_at(&mut working, &pointer)?;
        }
        *document = working;
        Ok(())
    }

    fn apply_at(
        &self,
        document: &mut Document,
        pointer: &crate::JsonPointer,
    ) -> Result<(), DomainError> {
        match self {
            Self::Set { value, .. } => document.set(pointer, value.clone()),
            Self::Clear { .. } => document.clear_path(pointer),
            Self::Insert { index, value, .. } => document.insert(pointer, *index, value.clone()),
            Self::Append { value, .. } => document.append(pointer, value.clone()),
        }
    }
}

/// Validates Document mutation values at paths declared by the package Schema.
pub fn validate_unified_action_schema(
    action: &UnifiedAction,
    schema: &DocumentSchemaNode,
) -> Result<(), DomainError> {
    let selected_nodes = match action {
        UnifiedAction::Document(DocumentMutation::Set { path, value }) => schema
            .resolve_match_path(path)
            .into_iter()
            .map(|node| (node, value.value_type()))
            .collect::<Vec<_>>(),
        UnifiedAction::Document(DocumentMutation::Clear { path, value_type }) => schema
            .resolve_match_path(path)
            .into_iter()
            .map(|node| (node, *value_type))
            .collect(),
        UnifiedAction::Document(
            DocumentMutation::Insert { path, value, .. } | DocumentMutation::Append { path, value },
        ) => schema
            .resolve_match_path(path)
            .into_iter()
            .map(|node| match node {
                DocumentSchemaNode::Array { items, .. } => Ok((items.as_ref(), value.value_type())),
                _ => Err(rule_error("action", "动作值类型与 Schema 声明不一致")),
            })
            .collect::<Result<Vec<_>, _>>()?,
        UnifiedAction::RecordMatch | UnifiedAction::Http(_) | UnifiedAction::Terminal(_) => {
            Vec::new()
        }
    };
    if selected_nodes
        .into_iter()
        .any(|(expected, value_type)| !expected.accepts(value_type))
    {
        return Err(rule_error("action", "动作值类型与 Schema 声明不一致"));
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
