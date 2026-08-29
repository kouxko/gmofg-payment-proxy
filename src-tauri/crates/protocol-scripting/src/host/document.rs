use std::{collections::BTreeMap, sync::Arc};

use intercept_proxy_domain::{
    Document, DocumentSchemaNode, DocumentValue, DomainError, ErrorCode, JsonPointer,
};
use rhai::{Array, Blob, Dynamic, Engine, EvalAltResult, ImmutableString, Map, Module, Position};

pub(super) fn register(engine: &mut Engine, schema: Arc<DocumentSchemaNode>) {
    let mut document_module = Module::new();
    document_module.set_native_fn("create", || {
        Ok(Document::new(DocumentValue::Object(BTreeMap::new())))
    });
    engine
        .register_type_with_name::<Document>("Document")
        .register_static_module("document", document_module.into())
        .register_fn("get", document_get)
        .register_fn(
            "set",
            move |document: &mut Document, path: &str, value: Dynamic| {
                document_set(document, &schema, path, value)
            },
        )
        .register_fn("has", document_has)
        .register_fn("fields", document_fields);
}

fn pointer(path: &str) -> Result<JsonPointer, Box<EvalAltResult>> {
    JsonPointer::parse(path).map_err(|error| domain_error_to_rhai(&error).into())
}
fn document_get(document: &mut Document, path: &str) -> Result<Dynamic, Box<EvalAltResult>> {
    document
        .resolve(&pointer(path)?)
        .map_err(|error| Box::new(domain_error_to_rhai(&error)))
        .and_then(document_value_to_dynamic)
}
fn document_set(
    document: &mut Document,
    schema: &DocumentSchemaNode,
    path: &str,
    value: Dynamic,
) -> Result<(), Box<EvalAltResult>> {
    let path = pointer(path)?;
    let schema = schema
        .resolve(&path)
        .map_err(|error| domain_error_to_rhai(&error))?;
    let value = dynamic_to_document_value(value)
        .ok_or_else(|| Box::new(host_error(ErrorCode::DocumentFieldTypeMismatch)))?;
    if !schema.accepts(value.value_type()) {
        return Err(Box::new(host_error(ErrorCode::DocumentFieldTypeMismatch)));
    }
    document
        .set(&path, value)
        .map_err(|error| domain_error_to_rhai(&error).into())
}
fn document_has(document: &mut Document, path: &str) -> Result<bool, Box<EvalAltResult>> {
    Ok(document.resolve(&pointer(path)?).is_ok())
}
fn document_fields(document: &mut Document) -> Result<Array, Box<EvalAltResult>> {
    match document.root() {
        DocumentValue::Object(values) => values
            .iter()
            .map(|(name, value)| {
                let mut field = Map::new();
                field.insert(
                    "name".into(),
                    Dynamic::from(ImmutableString::from(name.as_str())),
                );
                field.insert("value".into(), document_value_to_dynamic(value)?);
                Ok(Dynamic::from_map(field))
            })
            .collect(),
        _ => Ok(Array::new()),
    }
}
fn dynamic_to_document_value(value: Dynamic) -> Option<DocumentValue> {
    if value.is_unit() {
        Some(DocumentValue::null())
    } else if let Some(value) = value.clone().try_cast::<ImmutableString>() {
        Some(DocumentValue::String(value.into_owned()))
    } else if let Some(value) = value.clone().try_cast::<rhai::INT>() {
        DocumentValue::integer(value).ok()
    } else if let Some(value) = value.clone().try_cast::<bool>() {
        Some(DocumentValue::Boolean(value))
    } else if let Some(values) = value.clone().try_cast::<Blob>() {
        Some(DocumentValue::byte_array(values))
    } else if let Some(values) = value.clone().try_cast::<Array>() {
        values
            .into_iter()
            .map(dynamic_to_document_value)
            .collect::<Option<Vec<_>>>()
            .map(DocumentValue::Array)
    } else if let Some(values) = value.try_cast::<Map>() {
        values
            .into_iter()
            .map(|(key, value)| Some((key.into(), dynamic_to_document_value(value)?)))
            .collect::<Option<BTreeMap<_, _>>>()
            .map(DocumentValue::Object)
    } else {
        None
    }
}
fn document_value_to_dynamic(value: &DocumentValue) -> Result<Dynamic, Box<EvalAltResult>> {
    Ok(match value {
        DocumentValue::String(value) => Dynamic::from(ImmutableString::from(value.as_str())),
        DocumentValue::Number(value) => {
            let number = value.get();
            if number.fract() != 0.0 || number.abs() > 9_007_199_254_740_991.0 {
                return Err(Box::new(host_error(ErrorCode::DocumentNumberInvalid)));
            }
            #[allow(clippy::cast_possible_truncation)]
            Dynamic::from_int(number as rhai::INT)
        }
        DocumentValue::Boolean(value) => Dynamic::from_bool(*value),
        DocumentValue::Null(()) => Dynamic::UNIT,
        DocumentValue::Array(values) => Dynamic::from_array(
            values
                .iter()
                .map(document_value_to_dynamic)
                .collect::<Result<_, _>>()?,
        ),
        DocumentValue::Object(values) => Dynamic::from_map(
            values
                .iter()
                .map(|(key, value)| Ok((key.as_str().into(), document_value_to_dynamic(value)?)))
                .collect::<Result<_, Box<EvalAltResult>>>()?,
        ),
    })
}
fn domain_error_to_rhai(error: &DomainError) -> EvalAltResult {
    host_error(error.code)
}
fn host_error(code: ErrorCode) -> EvalAltResult {
    EvalAltResult::ErrorRuntime(code.as_str().into(), Position::NONE)
}
