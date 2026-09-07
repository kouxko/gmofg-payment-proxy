use serde_json::Value;

use crate::{route, signing};

pub(crate) fn downstream(input: &str) -> Result<String, String> {
    let mut http: Value =
        serde_json::from_str(input).map_err(|error| format!("invalid HTTP JSON: {error}"))?;
    validate_http(&http)?;
    let method = http["method"].as_str().expect("validated method");
    if method != "POST"
        || !http["code"]
            .as_u64()
            .is_some_and(|code| (200..300).contains(&code))
    {
        return Ok(input.to_owned());
    }
    let url = http["url"].as_str().expect("validated URL");
    let Some((operation, token)) = route::operation_and_token(url) else {
        return Ok(input.to_owned());
    };
    let body_text = http["body"].as_str().expect("validated body");
    let Ok(mut body) = serde_json::from_str::<Value>(body_text) else {
        return Ok(input.to_owned());
    };
    let Some(previous) = body.pointer("/data/sign").and_then(signing::java_string) else {
        return Ok(input.to_owned());
    };
    if previous.is_empty() {
        return Ok(input.to_owned());
    }
    let Some(sign) = signing::signature(&body, operation, &token) else {
        return Ok(input.to_owned());
    };
    if sign == previous {
        return Ok(input.to_owned());
    }
    // All applicability checks finish before mutating the candidate response.
    body["data"]["sign"] = Value::String(sign);
    http["body"] = Value::String(serde_json::to_string(&body).map_err(|error| error.to_string())?);
    serde_json::to_string(&http).map_err(|error| error.to_string())
}

fn validate_http(value: &Value) -> Result<(), String> {
    let Some(fields) = value.as_object() else {
        return Err("Finalize expects a five-field HTTP object".to_owned());
    };
    if fields.len() != 5
        || !["url", "method", "code", "header", "body"]
            .iter()
            .all(|name| fields.contains_key(*name))
        || !["url", "method", "body"]
            .iter()
            .all(|name| value[*name].is_string())
        || !(value["code"].is_null()
            || value["code"]
                .as_u64()
                .is_some_and(|code| u16::try_from(code).is_ok()))
        || !value["header"]
            .as_object()
            .is_some_and(|header| header.values().all(Value::is_string))
    {
        return Err("Finalize requires url/method/body strings, code number or null, and string-valued header object".to_owned());
    }
    Ok(())
}
