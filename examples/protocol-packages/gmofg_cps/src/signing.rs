use std::collections::BTreeMap;

use md5::{Digest, Md5};
use serde_json::{Map, Value};

use crate::route::Operation;

const BASE_RESULT: &[&str] = &["orderDetailId", "orderId", "transTime", "transStatus"];

pub(crate) fn signature(body: &Value, operation: Operation, token: &str) -> Option<String> {
    let text = canonical(body, operation, token)?;
    Some(format!("{:X}", Md5::digest(text.as_bytes())))
}

pub(crate) fn canonical(body: &Value, operation: Operation, token: &str) -> Option<String> {
    let root = body.as_object()?;
    let mut fields = BTreeMap::new();
    if let Some(data) = present(root, "data") {
        let data = data.as_object()?;
        let mut values = strings(data, &["errorCode", "errorInfo", "subErrorCode"])?;
        if let Some(result) = present(data, "result") {
            values.insert("result", result_pairs(result, operation)?);
        }
        fields.insert("data", format!("{{{}}}", pairs(values)));
    }
    if let Some(meta) = present(root, "meta") {
        let values = strings(meta.as_object()?, &["code", "message"])?;
        fields.insert("meta", format!("{{{}}}", pairs(values)));
    }
    // The App strips ASCII spaces from field values, but never from the token.
    Some(format!("{{{}&key=B2B{token}}}", pairs(fields)))
}

fn result_pairs(result: &Value, operation: Operation) -> Option<String> {
    let result = result.as_object()?;
    let mut fields = strings(result, BASE_RESULT)?;
    let (text, integers): (&[&str], &[&str]) = match operation {
        Operation::Pay => (
            &["currencyCode", "payType", "extendedInfo"],
            &["amount", "amountRmb"],
        ),
        Operation::Confirm => (&["payType", "payCheckDate"], &[]),
        Operation::Refund => (&["currencyCode"], &["refundAmount"]),
        Operation::Reverse => (&["currencyCode"], &["amount"]),
    };
    fields.extend(strings(result, text)?);
    for &name in integers {
        if let Some(value) = present(result, name) {
            let value = java_int(value)?;
            if value != 0 {
                fields.insert(name, value.to_string());
            }
        }
    }
    Some(format!("{{{}}}", pairs(fields)))
}

fn present<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    object.get(name).filter(|value| !value.is_null())
}

fn strings<'a>(
    object: &Map<String, Value>,
    names: &'a [&str],
) -> Option<BTreeMap<&'a str, String>> {
    let mut fields = BTreeMap::new();
    for &name in names {
        if let Some(value) = present(object, name) {
            fields.insert(name, java_string(value)?);
        }
    }
    Some(fields)
}

pub(crate) fn java_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn java_int(value: &Value) -> Option<i32> {
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(value) => value.to_string(),
        _ => return None,
    };
    if let Ok(number) = text.parse::<i32>() {
        return Some(number);
    }
    // Gson JsonReader.nextInt also accepts exactly integral decimal/scientific text.
    let number: f64 = text.trim_matches(|ch| ch <= '\u{0020}').parse().ok()?;
    if !number.is_finite() || number.fract() != 0.0 {
        return None;
    }
    format!("{number:.0}").parse().ok()
}

fn pairs(fields: BTreeMap<&str, String>) -> String {
    fields
        .into_iter()
        .map(|(name, value)| format!("{name}={}", value.replace(' ', "")))
        .collect::<Vec<_>>()
        .join("&")
}
