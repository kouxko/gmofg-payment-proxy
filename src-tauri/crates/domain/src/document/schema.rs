use super::{DocumentMatchPath, DocumentMatchToken, DocumentValueType, JsonPointer};
use crate::{DomainError, ErrorCode};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;

/// Schema 标题允许的最大 Unicode 字符数。
pub const MAX_DOCUMENT_SCHEMA_TITLE_CHARS: usize = 128;

/// Recursive, identity-free metadata describing a Document shape.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DocumentSchemaNode {
    /// String value metadata.
    String {
        /// Optional UI title.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    /// Number value metadata.
    Number {
        /// Optional UI title.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    /// Boolean value metadata.
    Boolean {
        /// Optional UI title.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    /// Object metadata keyed by JSON property name.
    Object {
        /// Optional UI title.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Per-property schema metadata.
        properties: BTreeMap<String, DocumentSchemaNode>,
    },
    /// Array metadata. Every array must declare its item schema.
    Array {
        /// Optional UI title.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Required item schema.
        items: Box<DocumentSchemaNode>,
    },
}

impl DocumentSchemaNode {
    /// Returns the optional UI-only title.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        match self {
            Self::String { title }
            | Self::Number { title }
            | Self::Boolean { title }
            | Self::Object { title, .. }
            | Self::Array { title, .. } => title.as_deref(),
        }
    }
    /// Validates schema metadata independently from a concrete Document value.
    pub fn validate_definition(&self) -> Result<(), DomainError> {
        self.validate_definition_at("")
    }
    /// Resolves schema metadata at an object-property path.
    pub fn resolve(&self, pointer: &JsonPointer) -> Result<&Self, DomainError> {
        let mut current = self;
        for token in pointer.tokens() {
            current = match current {
                Self::Object { properties, .. } => properties.get(token).ok_or_else(|| {
                    DomainError::new(ErrorCode::DocumentPathMissing, "schema path is missing")
                        .with_field_error(pointer.as_str(), "schema path is missing")
                })?,
                Self::Array { items, .. } if valid_array_index(token) => items,
                Self::Array { .. } => {
                    return Err(DomainError::new(
                        ErrorCode::DocumentPathMissing,
                        "schema path contains an invalid array index",
                    )
                    .with_field_error(pointer.as_str(), "invalid array index"));
                }
                _ => {
                    return Err(DomainError::new(
                        ErrorCode::DocumentPathTypeMismatch,
                        "schema path traverses a scalar",
                    )
                    .with_field_error(pointer.as_str(), "schema path traverses a scalar"));
                }
            };
        }
        Ok(current)
    }
    /// Resolves every schema node selected by a condition or action wildcard path.
    #[must_use]
    pub fn resolve_match_path(&self, path: &DocumentMatchPath) -> Vec<&Self> {
        let mut current = vec![self];
        for token in path.tokens() {
            let mut next = Vec::new();
            for node in current {
                match (token, node) {
                    (DocumentMatchToken::Wildcard, Self::Object { properties, .. }) => {
                        next.extend(properties.values());
                    }
                    (DocumentMatchToken::Wildcard, Self::Array { items, .. }) => {
                        next.push(items.as_ref());
                    }
                    (DocumentMatchToken::Exact(token), Self::Object { properties, .. }) => {
                        if let Some(child) = properties.get(token) {
                            next.push(child);
                        }
                    }
                    (DocumentMatchToken::Exact(token), Self::Array { items, .. })
                        if valid_array_index(token) =>
                    {
                        next.push(items.as_ref());
                    }
                    (DocumentMatchToken::Wildcard | DocumentMatchToken::Exact(_), _) => {}
                }
            }
            if next.is_empty() {
                return next;
            }
            current = next;
        }
        current
    }
    /// Returns whether this schema node accepts the given JSON value kind.
    #[must_use]
    pub const fn accepts(&self, kind: DocumentValueType) -> bool {
        matches!(
            (self, kind),
            (Self::String { .. }, DocumentValueType::String)
                | (Self::Number { .. }, DocumentValueType::Number)
                | (Self::Boolean { .. }, DocumentValueType::Boolean)
                | (Self::Object { .. }, DocumentValueType::Object)
                | (Self::Array { .. }, DocumentValueType::Array)
        )
    }
    fn validate_definition_at(&self, path: &str) -> Result<(), DomainError> {
        if self.title().is_some_and(|title| {
            title.trim().is_empty() || title.chars().count() > MAX_DOCUMENT_SCHEMA_TITLE_CHARS
        }) {
            return Err(DomainError::new(
                ErrorCode::DocumentSchemaInvalid,
                "schema title must contain 1 to 128 visible characters",
            )
            .with_field_error(path, "invalid title"));
        }
        match self {
            Self::Object { properties, .. } => {
                for (name, child) in properties {
                    child.validate_definition_at(&pointer_child(path, name))?;
                }
                Ok(())
            }
            Self::Array { items, .. } => items.validate_definition_at(&format!("{path}/0")),
            Self::String { .. } | Self::Number { .. } | Self::Boolean { .. } => Ok(()),
        }
    }
}
fn valid_array_index(token: &str) -> bool {
    !token.is_empty()
        && (token.len() == 1 || !token.starts_with('0'))
        && token.bytes().all(|byte| byte.is_ascii_digit())
}
fn pointer_child(parent: &str, name: &str) -> String {
    format!("{parent}/{}", name.replace('~', "~0").replace('/', "~1"))
}
