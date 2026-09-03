use std::fmt::Write as _;

use serde_json::{Map, Value};

use crate::model::DllDocument;

pub(crate) fn downstream(document: &DllDocument) -> Result<String, String> {
    let value = serde_json::to_value(document)
        .map_err(|error| format!("cannot serialize GMO-FG Payment DLL display: {error}"))?;
    Ok(render("GMO-FG Payment DLL downstream", &value))
}

pub(crate) fn upstream(document: &Value) -> String {
    render("GMO-FG Payment DLL upstream passthrough", document)
}

fn render(title: &str, value: &Value) -> String {
    let mut html = String::from(
        "<section style=\"font-family:ui-monospace,SFMono-Regular,Menlo,monospace;white-space:pre-wrap;overflow-wrap:anywhere\"><h3>",
    );
    html.push_str(&escape_html(title));
    html.push_str("</h3>");
    let mut table_index = 0;
    render_node(value, "基本信息", true, &mut html, &mut table_index);
    html.push_str("</section>");
    html
}

fn render_node(
    value: &Value,
    label: &str,
    is_root: bool,
    output: &mut String,
    table_index: &mut usize,
) {
    match value {
        Value::Object(object) => render_object(object, label, is_root, output, table_index),
        Value::Array(array) => render_array(array, label, is_root, output, table_index),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            table_start(output, table_index);
            output.push_str("<thead><tr><th>Value</th></tr></thead><tbody><tr><td>");
            render_scalar(value, output);
            output.push_str("</td></tr></tbody></table>");
        }
    }
}

fn render_object(
    object: &Map<String, Value>,
    label: &str,
    is_root: bool,
    output: &mut String,
    table_index: &mut usize,
) {
    disclosure_start(label, "Object", object.len(), "fields", is_root, output);

    let scalar_fields: Vec<_> = object
        .iter()
        .filter(|(_, value)| is_scalar(value))
        .collect();
    if !scalar_fields.is_empty() {
        table_start(output, table_index);
        output.push_str("<thead><tr>");
        for (field, _) in &scalar_fields {
            output.push_str("<th>");
            output.push_str(&escape_html(field));
            output.push_str("</th>");
        }
        output.push_str("</tr></thead><tbody><tr>");
        for (_, value) in &scalar_fields {
            output.push_str("<td>");
            render_scalar(value, output);
            output.push_str("</td>");
        }
        output.push_str("</tr></tbody></table>");
    }

    for (field, value) in object.iter().filter(|(_, value)| !is_scalar(value)) {
        render_node(value, field, false, output, table_index);
    }

    output.push_str("</details>");
}

fn render_array(
    array: &[Value],
    label: &str,
    is_root: bool,
    output: &mut String,
    table_index: &mut usize,
) {
    disclosure_start(label, "Array", array.len(), "items", is_root, output);

    if array.is_empty() {
        output.push_str("<p>Empty Array</p></details>");
        return;
    }

    let mut scalar_items = Vec::new();
    for (index, value) in array.iter().enumerate() {
        match value {
            Value::Object(_) | Value::Array(_) => {
                render_node(value, &format!("[{index}]"), false, output, table_index);
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                scalar_items.push((index, value));
            }
        }
    }

    if !scalar_items.is_empty() {
        table_start(output, table_index);
        output.push_str("<thead><tr><th>Index</th><th>Value</th></tr></thead><tbody>");
        for (index, value) in scalar_items {
            output.push_str("<tr><td>");
            output.push_str(&index.to_string());
            output.push_str("</td><td>");
            render_scalar(value, output);
            output.push_str("</td></tr>");
        }
        output.push_str("</tbody></table>");
    }

    output.push_str("</details>");
}

fn disclosure_start(
    label: &str,
    kind: &str,
    count: usize,
    count_label: &str,
    open: bool,
    output: &mut String,
) {
    output.push_str(if open { "<details open>" } else { "<details>" });
    output.push_str("<summary><strong>");
    output.push_str(&escape_html(label));
    output.push_str("</strong><span>");
    write!(output, "{kind} · {count} {count_label}")
        .expect("writing JSON tree metadata to String cannot fail");
    output.push_str("</span></summary>");
}

fn table_start(output: &mut String, table_index: &mut usize) {
    let hue = (*table_index * 137 + 18) % 360;
    let saturation = 45 + ((*table_index / 360) % 6) * 5;
    let lightness = 46 + ((*table_index / 2_160) % 4) * 3;
    *table_index += 1;

    write!(
        output,
        "<table style=\"background-color:hsl({hue} {saturation}% {lightness}% / 0.10);border-color:hsl({hue} 65% 50%)\">"
    )
    .expect("writing table theme to String cannot fail");
}

fn render_scalar(value: &Value, output: &mut String) {
    output.push_str(&escape_html(&scalar_text(value)));
}

fn is_scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => "null".to_owned(),
        Value::Array(_) | Value::Object(_) => {
            unreachable!("container values do not have scalar text")
        }
    }
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}
