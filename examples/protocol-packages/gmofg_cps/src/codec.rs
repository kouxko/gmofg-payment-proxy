use serde_json::Value;

pub(crate) fn decode(input: &str) -> Result<String, String> {
    // D6: a non-JSON business body must reach Finalize unchanged as a text Document.
    let document =
        serde_json::from_str::<Value>(input).unwrap_or_else(|_| Value::String(input.to_owned()));
    serde_json::to_string(&document).map_err(|error| error.to_string())
}

pub(crate) fn encode(original_input: &str, document_json: &str) -> Result<String, String> {
    let document: Value = serde_json::from_str(document_json)
        .map_err(|error| format!("invalid CPS Document JSON: {error}"))?;
    match serde_json::from_str::<Value>(original_input) {
        Ok(original) if json_equivalent(&original, &document) => Ok(original_input.to_owned()),
        Ok(_) => serde_json::to_string_pretty(&document).map_err(|error| error.to_string()),
        Err(_) => match document {
            Value::String(text) => Ok(text),
            value => serde_json::to_string_pretty(&value).map_err(|error| error.to_string()),
        },
    }
}

// Match the existing json_pretty codec: Host Document stores numbers as f64.
// Compare its numeric representation so a no-op does not rewrite the wire body.
fn json_equivalent(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => match (left.as_f64(), right.as_f64()) {
            (Some(left), Some(right)) => left == right,
            _ => left == right,
        },
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| json_equivalent(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, left)| {
                    right
                        .get(key)
                        .is_some_and(|right| json_equivalent(left, right))
                })
        }
        _ => left == right,
    }
}

pub(crate) fn display(document_json: &str) -> Result<String, String> {
    let document: Value = serde_json::from_str(document_json)
        .map_err(|error| format!("invalid CPS Display Document: {error}"))?;
    let text = match &document {
        Value::String(text) => text.clone(),
        _ => serde_json::to_string_pretty(&document).map_err(|error| error.to_string())?,
    };
    let mut html = String::from(
        "<section><h3>GMOFG CPS</h3><pre style=\"white-space:pre-wrap;overflow-wrap:anywhere\">",
    );
    for ch in text.chars() {
        match ch {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            '"' => html.push_str("&quot;"),
            '\'' => html.push_str("&#39;"),
            _ => html.push(ch),
        }
    }
    html.push_str("</pre></section>");
    Ok(html)
}
