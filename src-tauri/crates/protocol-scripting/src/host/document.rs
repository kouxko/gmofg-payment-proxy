use std::sync::Arc;

use intercept_proxy_domain::{
    Document, DocumentFieldType, DocumentSchema, DocumentValue, DomainError, ErrorCode,
};
use rhai::{Array, Blob, Dynamic, Engine, EvalAltResult, ImmutableString, Map, Module, Position};

pub(super) fn register(engine: &mut Engine, schema: Arc<DocumentSchema>) {
    let mut document_module = Module::new();
    document_module.set_native_fn("create", move || Ok(Document::new(Arc::clone(&schema))));

    engine
        .register_type_with_name::<Document>("Document")
        .register_static_module("document", document_module.into())
        .register_fn("get", document_get)
        .register_fn("set", document_set)
        .register_fn("has", document_has)
        .register_fn("fields", document_fields);
}

fn document_get(document: &mut Document, name: &str) -> Result<Dynamic, Box<EvalAltResult>> {
    document
        .get(name)
        .map(document_value_to_dynamic)
        .map_err(|error| domain_error_to_rhai(&error).into())
}

fn document_set(
    document: &mut Document,
    name: &str,
    value: Dynamic,
) -> Result<(), Box<EvalAltResult>> {
    let field_type = document
        .schema()
        .field_index(name)
        .map(|index| document.schema().fields()[index].field_type())
        .ok_or_else(|| Box::new(host_error(ErrorCode::DocumentFieldUndeclared)))?;
    let value = dynamic_to_document_value(field_type, value)
        .ok_or_else(|| Box::new(host_error(ErrorCode::DocumentFieldTypeMismatch)))?;
    document
        .set(name, value)
        .map_err(|error| domain_error_to_rhai(&error).into())
}

fn document_has(document: &mut Document, name: &str) -> Result<bool, Box<EvalAltResult>> {
    document
        .has(name)
        .map_err(|error| domain_error_to_rhai(&error).into())
}

fn document_fields(document: &mut Document) -> Array {
    document
        .fields()
        .map(|state| {
            let mut field = Map::new();
            field.insert(
                "name".into(),
                Dynamic::from(ImmutableString::from(state.field.name().as_str())),
            );
            field.insert(
                "label".into(),
                Dynamic::from(ImmutableString::from(state.field.label())),
            );
            field.insert(
                "type".into(),
                Dynamic::from(ImmutableString::from(state.field.field_type().as_str())),
            );
            field.insert("present".into(), Dynamic::from_bool(state.value.is_some()));
            field.insert(
                "value".into(),
                state.value.map_or(Dynamic::UNIT, document_value_to_dynamic),
            );
            Dynamic::from_map(field)
        })
        .collect()
}

fn dynamic_to_document_value(
    field_type: DocumentFieldType,
    value: Dynamic,
) -> Option<DocumentValue> {
    match field_type {
        DocumentFieldType::String => value
            .try_cast::<ImmutableString>()
            .map(|value| DocumentValue::String(value.into_owned())),
        DocumentFieldType::Int => value.try_cast::<rhai::INT>().map(DocumentValue::Int),
        DocumentFieldType::Bool => value.try_cast::<bool>().map(DocumentValue::Bool),
        DocumentFieldType::Blob => value.try_cast::<Blob>().map(DocumentValue::Blob),
    }
}

fn document_value_to_dynamic(value: &DocumentValue) -> Dynamic {
    match value {
        DocumentValue::String(value) => Dynamic::from(ImmutableString::from(value.as_str())),
        DocumentValue::Int(value) => Dynamic::from_int(*value),
        DocumentValue::Bool(value) => Dynamic::from_bool(*value),
        DocumentValue::Blob(value) => Dynamic::from_blob(value.clone()),
    }
}

fn domain_error_to_rhai(error: &DomainError) -> EvalAltResult {
    host_error(error.code)
}

fn host_error(code: ErrorCode) -> EvalAltResult {
    EvalAltResult::ErrorRuntime(code.as_str().into(), Position::NONE)
}
