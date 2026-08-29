use crate::{Document, DocumentValue};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;

/// Standard recursive JSON Document at the external-package boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(transparent)]
pub struct ExternalDocumentWire(DocumentValue);

impl Default for ExternalDocumentWire {
    fn default() -> Self {
        Self(DocumentValue::Object(BTreeMap::new()))
    }
}

impl ExternalDocumentWire {
    /// Converts the standard JSON wire into an owned Document.
    #[must_use]
    pub fn into_document(self) -> Document {
        Document::new(self.0)
    }
    /// Copies a domain Document into the standard JSON wire.
    #[must_use]
    pub fn from_document(document: &Document) -> Self {
        Self(document.root().clone())
    }
}
