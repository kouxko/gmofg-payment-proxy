use crate::{DomainError, ErrorCode};
use serde::{Deserialize, Serialize};
use specta::Type;

/// Parsed RFC 6901 JSON Pointer. The document root is the empty string.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type)]
#[serde(transparent)]
pub struct JsonPointer {
    source: String,
    #[serde(skip)]
    tokens: Vec<String>,
}

/// Condition-only RFC 6901 path whose `*` token selects exactly one object/array level.
///
/// Mutation APIs deliberately continue to accept only [`JsonPointer`].
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type)]
#[serde(transparent)]
pub struct DocumentMatchPath {
    source: String,
    #[serde(skip)]
    tokens: Vec<DocumentMatchToken>,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum DocumentMatchToken {
    Exact(String),
    Wildcard,
}

impl JsonPointer {
    /// Returns the root pointer.
    #[must_use]
    pub fn root() -> Self {
        Self {
            source: String::new(),
            tokens: Vec::new(),
        }
    }
    /// Creates a pointer for one object property.
    #[must_use]
    pub fn property(name: &str) -> Self {
        Self::parse(&format!("/{}", name.replace('~', "~0").replace('/', "~1")))
            .expect("encoded property is valid")
    }
    /// Parses and validates an RFC 6901 pointer.
    pub fn parse(source: &str) -> Result<Self, DomainError> {
        if source.is_empty() {
            return Ok(Self::root());
        }
        if !source.starts_with('/') {
            return Err(invalid(
                source,
                "JSON Pointer must be empty or start with '/'",
            ));
        }
        let tokens = source[1..]
            .split('/')
            .map(|token| decode(token, source))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            source: source.to_owned(),
            tokens,
        })
    }
    /// Returns canonical pointer text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.source
    }
    /// Returns UI text, displaying root as `/`.
    #[must_use]
    pub fn ui_path(&self) -> &str {
        if self.source.is_empty() {
            "/"
        } else {
            &self.source
        }
    }
    pub(crate) fn tokens(&self) -> &[String] {
        &self.tokens
    }
}

impl DocumentMatchPath {
    /// Parses an RFC 6901 path extended only by a complete `*` path token.
    pub fn parse(source: &str) -> Result<Self, DomainError> {
        let pointer = JsonPointer::parse(source)?;
        let tokens = pointer
            .tokens()
            .iter()
            .map(|token| {
                if token == "*" {
                    DocumentMatchToken::Wildcard
                } else {
                    DocumentMatchToken::Exact(token.clone())
                }
            })
            .collect();
        Ok(Self {
            source: source.to_owned(),
            tokens,
        })
    }

    /// Converts an exact mutation/schema pointer into a condition path.
    #[must_use]
    pub fn exact(pointer: &JsonPointer) -> Self {
        Self {
            source: pointer.as_str().to_owned(),
            tokens: pointer
                .tokens()
                .iter()
                .cloned()
                .map(DocumentMatchToken::Exact)
                .collect(),
        }
    }

    /// Returns the stable serialized condition path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Returns whether this condition path expands any level.
    #[must_use]
    pub fn has_wildcard(&self) -> bool {
        self.tokens
            .iter()
            .any(|token| matches!(token, DocumentMatchToken::Wildcard))
    }

    pub(crate) fn tokens(&self) -> &[DocumentMatchToken] {
        &self.tokens
    }
}
impl<'de> Deserialize<'de> for JsonPointer {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let source = String::deserialize(deserializer)?;
        Self::parse(&source).map_err(serde::de::Error::custom)
    }
}
impl<'de> Deserialize<'de> for DocumentMatchPath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let source = String::deserialize(deserializer)?;
        Self::parse(&source).map_err(serde::de::Error::custom)
    }
}
fn decode(token: &str, source: &str) -> Result<String, DomainError> {
    let mut decoded = String::new();
    let mut chars = token.chars();
    while let Some(ch) = chars.next() {
        if ch == '~' {
            match chars.next() {
                Some('0') => decoded.push('~'),
                Some('1') => decoded.push('/'),
                _ => return Err(invalid(source, "JSON Pointer contains invalid '~' escape")),
            }
        } else {
            decoded.push(ch);
        }
    }
    Ok(decoded)
}
fn invalid(path: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::DocumentPointerInvalid, message).with_field_error(path, message)
}
