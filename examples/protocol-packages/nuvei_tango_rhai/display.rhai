fn escape_html(value) {
    let escaped = value;
    escaped.replace("&", "&amp;");
    escaped.replace("<", "&lt;");
    escaped.replace(">", "&gt;");
    escaped.replace("\"", "&quot;");
    escaped.replace("'", "&#39;");
    escaped
}

fn require_string(document, field) {
    if !document.has(field) {
        throw "Nuvei Tango read-only document is missing a display field";
    }
    document.get(field)
}

fn display_message(document, context) {
    let label = if context.direction() == "upstream" { "Upstream" } else { "Downstream" };
    let sequence = escape_html(require_string(document, "/sequence"));
    let message_type = escape_html(require_string(document, "/message_type"));
    let json_preview = escape_html(require_string(document, "/json_preview"));

    `<section class="protocol-document"><h3>Nuvei Tango JSON</h3><table><tbody><tr><th>Direction</th><td>${label}</td></tr><tr><th>Sequence</th><td>${sequence}</td></tr><tr><th>Message type</th><td>${message_type}</td></tr></tbody></table><pre>${json_preview}</pre></section>`
}

fn display_upstream(document, context) { display_message(document, context) }
fn display_downstream(document, context) { display_message(document, context) }
