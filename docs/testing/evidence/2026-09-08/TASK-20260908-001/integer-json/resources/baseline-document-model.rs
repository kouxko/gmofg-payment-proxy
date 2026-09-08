use super::{DocumentMatchPath, DocumentMatchToken, JsonPointer};
use crate::{DomainError, ErrorCode};
use serde::{Deserialize, Deserializer, Serialize, de::Visitor};
use specta::Type;
use std::{collections::BTreeMap, fmt};

/// JavaScript `Number` compatible finite numeric value.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Type)]
#[specta(type = specta_typescript::Number)]
pub struct DocumentNumber(f64);

impl Eq for DocumentNumber {}

impl DocumentNumber {
    /// Largest integer that JavaScript can represent exactly.
    pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
    const MAX_SAFE_INTEGER_F64: f64 = 9_007_199_254_740_991.0;
    /// Creates a finite number.
    pub fn new(value: f64) -> Result<Self, DomainError> {
        if !value.is_finite() {
            return Err(DomainError::new(
                ErrorCode::DocumentNumberInvalid,
                "Document number must be finite",
            ));
        }
        if value.fract() == 0.0 && value.abs() > Self::MAX_SAFE_INTEGER_F64 {
            return Err(DomainError::new(
                ErrorCode::DocumentUnsafeInteger,
                "integer exceeds JavaScript safe integer range",
            ));
        }
        Ok(Self(value))
    }
    /// Returns the validated number.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    #[allow(clippy::cast_precision_loss)]
    const fn from_safe_i64(value: i64) -> Self {
        Self(value as f64)
    }

    #[allow(clippy::cast_precision_loss)]
    const fn from_safe_u64(value: u64) -> Self {
        Self(value as f64)
    }
}

impl<'de> Deserialize<'de> for DocumentNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NumberVisitor;
        impl Visitor<'_> for NumberVisitor {
            type Value = DocumentNumber;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a finite JavaScript number")
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                if value.unsigned_abs() > DocumentNumber::MAX_SAFE_INTEGER as u64 {
                    return Err(E::custom("DOCUMENT_UNSAFE_INTEGER"));
                }
                Ok(DocumentNumber::from_safe_i64(value))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                if value > DocumentNumber::MAX_SAFE_INTEGER as u64 {
                    return Err(E::custom("DOCUMENT_UNSAFE_INTEGER"));
                }
                Ok(DocumentNumber::from_safe_u64(value))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                DocumentNumber::new(value).map_err(E::custom)
            }
        }
        deserializer.deserialize_any(NumberVisitor)
    }
}

/// Recursive JSON value owned by a [`Document`].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(untagged)]
pub enum DocumentValue {
    /// JSON string.
    String(String),
    /// Finite JavaScript number.
    Number(DocumentNumber),
    /// JSON boolean.
    Boolean(bool),
    /// JSON null.
    Null(()),
    /// JSON object. Key order is not semantic.
    Object(BTreeMap<String, DocumentValue>),
    /// JSON array.
    Array(Vec<DocumentValue>),
}

impl Eq for DocumentValue {}

/// Schema-visible kind of a document value.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DocumentValueType {
    /// String.
    String,
    /// Number.
    Number,
    /// Boolean.
    Boolean,
    /// Null.
    Null,
    /// Object.
    Object,
    /// Array.
    Array,
}

impl DocumentValue {
    /// Creates the JSON null value.
    #[must_use]
    pub const fn null() -> Self {
        Self::Null(())
    }

    /// Returns whether this value is JSON null.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null(()))
    }

    /// Creates a Number from an exactly representable signed integer.
    pub fn integer(value: i64) -> Result<Self, DomainError> {
        if value.unsigned_abs() > DocumentNumber::MAX_SAFE_INTEGER as u64 {
            return Err(DomainError::new(
                ErrorCode::DocumentUnsafeInteger,
                "integer exceeds JavaScript safe integer range",
            ));
        }
        Ok(Self::Number(DocumentNumber::from_safe_i64(value)))
    }

    /// Represents bytes as a JSON array of numeric octets.
    #[must_use]
    pub fn byte_array(values: impl IntoIterator<Item = u8>) -> Self {
        Self::Array(
            values
                .into_iter()
                .map(|value| Self::Number(DocumentNumber(f64::from(value))))
                .collect(),
        )
    }

    /// Returns this value's JSON kind.
    #[must_use]
    pub const fn value_type(&self) -> DocumentValueType {
        match self {
            Self::String(_) => DocumentValueType::String,
            Self::Number(_) => DocumentValueType::Number,
            Self::Boolean(_) => DocumentValueType::Boolean,
            Self::Null(()) => DocumentValueType::Null,
            Self::Object(_) => DocumentValueType::Object,
            Self::Array(_) => DocumentValueType::Array,
        }
    }
}

/// Identity-free, owned recursive JSON document.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(transparent)]
pub struct Document(DocumentValue);

impl Eq for Document {}

impl Document {
    /// Creates a document from its root value.
    #[must_use]
    pub const fn new(root: DocumentValue) -> Self {
        Self(root)
    }
    /// Parses standard JSON under the JavaScript-number contract.
    pub fn parse_json(input: &str) -> Result<Self, DomainError> {
        let value: serde_json::Value = serde_json::from_str(input).map_err(|error| {
            DomainError::new(
                ErrorCode::JsonInvalid,
                format!("Invalid Document JSON: {error}"),
            )
        })?;
        Ok(Self(from_json_value(value)?))
    }
    /// Serializes this document as standard JSON.
    pub fn to_json(&self) -> Result<String, DomainError> {
        serde_json::to_string(self).map_err(|error| {
            DomainError::new(
                ErrorCode::JsonInvalid,
                format!("Cannot encode Document JSON: {error}"),
            )
        })
    }
    /// Borrows the root value.
    #[must_use]
    pub const fn root(&self) -> &DocumentValue {
        &self.0
    }
    /// Resolves an RFC 6901 pointer.
    pub fn resolve(&self, pointer: &JsonPointer) -> Result<&DocumentValue, DomainError> {
        resolve_tokens(&self.0, pointer.tokens(), pointer.as_str())
    }
    /// Resolves a condition or action path. Each wildcard expands one object/array level.
    #[must_use]
    pub fn resolve_match_path(&self, path: &DocumentMatchPath) -> Vec<&DocumentValue> {
        let mut current = vec![&self.0];
        for token in path.tokens() {
            let mut next = Vec::new();
            for value in current {
                match (token, value) {
                    (DocumentMatchToken::Wildcard, DocumentValue::Object(values)) => {
                        next.extend(values.values());
                    }
                    (DocumentMatchToken::Wildcard, DocumentValue::Array(values)) => {
                        next.extend(values.iter());
                    }
                    (DocumentMatchToken::Exact(token), DocumentValue::Object(values)) => {
                        if let Some(value) = values.get(token) {
                            next.push(value);
                        }
                    }
                    (DocumentMatchToken::Exact(token), DocumentValue::Array(values)) => {
                        if let Ok(index) = token.parse::<usize>()
                            && token == &index.to_string()
                            && let Some(value) = values.get(index)
                        {
                            next.push(value);
                        }
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
    /// Resolves a wildcard path into concrete pointers using the current document snapshot.
    #[must_use]
    pub fn resolve_match_pointers(&self, path: &DocumentMatchPath) -> Vec<JsonPointer> {
        let mut current = vec![(&self.0, Vec::<String>::new())];
        for token in path.tokens() {
            let mut next = Vec::new();
            for (value, tokens) in current {
                match (token, value) {
                    (DocumentMatchToken::Wildcard, DocumentValue::Object(values)) => {
                        next.extend(values.iter().map(|(name, value)| {
                            let mut child = tokens.clone();
                            child.push(name.clone());
                            (value, child)
                        }));
                    }
                    (DocumentMatchToken::Wildcard, DocumentValue::Array(values)) => {
                        next.extend(values.iter().enumerate().map(|(index, value)| {
                            let mut child = tokens.clone();
                            child.push(index.to_string());
                            (value, child)
                        }));
                    }
                    (DocumentMatchToken::Exact(token), DocumentValue::Object(values)) => {
                        if let Some(value) = values.get(token) {
                            let mut child = tokens;
                            child.push(token.clone());
                            next.push((value, child));
                        }
                    }
                    (DocumentMatchToken::Exact(token), DocumentValue::Array(values)) => {
                        if let Ok(index) = token.parse::<usize>()
                            && token == &index.to_string()
                            && let Some(value) = values.get(index)
                        {
                            let mut child = tokens;
                            child.push(token.clone());
                            next.push((value, child));
                        }
                    }
                    (DocumentMatchToken::Wildcard | DocumentMatchToken::Exact(_), _) => {}
                }
            }
            if next.is_empty() {
                return Vec::new();
            }
            current = next;
        }
        current
            .into_iter()
            .map(|(_, tokens)| JsonPointer::from_tokens(tokens))
            .collect()
    }
    /// Replaces root/existing node, or adds a final object property when its parent exists.
    pub fn set(&mut self, pointer: &JsonPointer, value: DocumentValue) -> Result<(), DomainError> {
        if pointer.tokens().is_empty() {
            self.0 = value;
            return Ok(());
        }
        let (parents, final_token) = pointer.tokens().split_at(pointer.tokens().len() - 1);
        match resolve_tokens_mut(&mut self.0, parents, pointer.as_str())? {
            DocumentValue::Object(values) => {
                values.insert(final_token[0].clone(), value);
                Ok(())
            }
            DocumentValue::Array(values) => {
                let index = array_index(&final_token[0], values.len(), pointer.as_str())?;
                values[index] = value;
                Ok(())
            }
            _ => Err(type_error(
                pointer.as_str(),
                "set parent must be object or array",
            )),
        }
    }
    /// Removes an object property or array item. Root cannot be cleared.
    pub fn clear_path(&mut self, pointer: &JsonPointer) -> Result<(), DomainError> {
        if pointer.tokens().is_empty() {
            return Err(path_error(
                pointer.as_str(),
                "document root cannot be cleared",
            ));
        }
        let (parents, final_token) = pointer.tokens().split_at(pointer.tokens().len() - 1);
        match resolve_tokens_mut(&mut self.0, parents, pointer.as_str())? {
            DocumentValue::Object(values) => values
                .remove(&final_token[0])
                .map(|_| ())
                .ok_or_else(|| path_error(pointer.as_str(), "object property is missing")),
            DocumentValue::Array(values) => {
                let index = array_index(&final_token[0], values.len(), pointer.as_str())?;
                values.remove(index);
                Ok(())
            }
            _ => Err(type_error(
                pointer.as_str(),
                "clear parent must be object or array",
            )),
        }
    }
    /// Inserts a value into an existing array at `0..=len`.
    pub fn insert(
        &mut self,
        pointer: &JsonPointer,
        index: usize,
        value: DocumentValue,
    ) -> Result<(), DomainError> {
        match resolve_tokens_mut(&mut self.0, pointer.tokens(), pointer.as_str())? {
            DocumentValue::Array(values) if index <= values.len() => {
                values.insert(index, value);
                Ok(())
            }
            DocumentValue::Array(_) => Err(path_error(
                pointer.as_str(),
                "array insert index is out of range",
            )),
            _ => Err(type_error(
                pointer.as_str(),
                "insert target must be an array",
            )),
        }
    }
    /// Appends a value to an existing array.
    pub fn append(
        &mut self,
        pointer: &JsonPointer,
        value: DocumentValue,
    ) -> Result<(), DomainError> {
        match resolve_tokens_mut(&mut self.0, pointer.tokens(), pointer.as_str())? {
            DocumentValue::Array(values) => {
                values.push(value);
                Ok(())
            }
            _ => Err(type_error(
                pointer.as_str(),
                "append target must be an array",
            )),
        }
    }
}

fn from_json_value(value: serde_json::Value) -> Result<DocumentValue, DomainError> {
    Ok(match value {
        serde_json::Value::String(value) => DocumentValue::String(value),
        serde_json::Value::Bool(value) => DocumentValue::Boolean(value),
        serde_json::Value::Null => DocumentValue::null(),
        serde_json::Value::Array(values) => DocumentValue::Array(
            values
                .into_iter()
                .map(from_json_value)
                .collect::<Result<_, _>>()?,
        ),
        serde_json::Value::Object(values) => DocumentValue::Object(
            values
                .into_iter()
                .map(|(key, value)| Ok((key, from_json_value(value)?)))
                .collect::<Result<_, DomainError>>()?,
        ),
        serde_json::Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                if integer.unsigned_abs() > DocumentNumber::MAX_SAFE_INTEGER as u64 {
                    return Err(DomainError::new(
                        ErrorCode::DocumentUnsafeInteger,
                        "integer exceeds JavaScript safe integer range",
                    ));
                }
                DocumentValue::Number(DocumentNumber::from_safe_i64(integer))
            } else if let Some(integer) = value.as_u64() {
                if integer > DocumentNumber::MAX_SAFE_INTEGER as u64 {
                    return Err(DomainError::new(
                        ErrorCode::DocumentUnsafeInteger,
                        "integer exceeds JavaScript safe integer range",
                    ));
                }
                DocumentValue::Number(DocumentNumber::from_safe_u64(integer))
            } else {
                DocumentValue::Number(DocumentNumber::new(value.as_f64().ok_or_else(|| {
                    DomainError::new(ErrorCode::DocumentNumberInvalid, "invalid JSON number")
                })?)?)
            }
        }
    })
}

fn array_index(token: &str, len: usize, path: &str) -> Result<usize, DomainError> {
    if token.is_empty()
        || (token.len() > 1 && token.starts_with('0'))
        || !token.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(path_error(path, "invalid array index"));
    }
    let index = token
        .parse::<usize>()
        .map_err(|_| path_error(path, "invalid array index"))?;
    (index < len)
        .then_some(index)
        .ok_or_else(|| path_error(path, "array index is out of range"))
}
fn resolve_tokens<'a>(
    mut value: &'a DocumentValue,
    tokens: &[String],
    path: &str,
) -> Result<&'a DocumentValue, DomainError> {
    for token in tokens {
        value = match value {
            DocumentValue::Object(values) => values
                .get(token)
                .ok_or_else(|| path_error(path, "object property is missing"))?,
            DocumentValue::Array(values) => &values[array_index(token, values.len(), path)?],
            _ => return Err(type_error(path, "path traverses a scalar")),
        };
    }
    Ok(value)
}
fn resolve_tokens_mut<'a>(
    mut value: &'a mut DocumentValue,
    tokens: &[String],
    path: &str,
) -> Result<&'a mut DocumentValue, DomainError> {
    for token in tokens {
        value = match value {
            DocumentValue::Object(values) => values
                .get_mut(token)
                .ok_or_else(|| path_error(path, "object property is missing"))?,
            DocumentValue::Array(values) => {
                let index = array_index(token, values.len(), path)?;
                &mut values[index]
            }
            _ => return Err(type_error(path, "path traverses a scalar")),
        };
    }
    Ok(value)
}
fn path_error(path: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::DocumentPathMissing, message).with_field_error(path, message)
}
fn type_error(path: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::DocumentPathTypeMismatch, message).with_field_error(path, message)
}
