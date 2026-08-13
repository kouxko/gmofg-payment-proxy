// 可选入口：只负责把 Document 生成用于应用内嵌展示的 HTML。
// 没有此入口、Listener 关闭显示能力或脚本执行失败时，应用显示 Frame Hex。
// 返回值按不可信 HTML 处理；宿主会清洗脚本、事件属性和外部资源。

fn field_row(document, field_name, label) {
    if !document.has(field_name) {
        return "";
    }

    `<tr><th>${label}</th><td>${document.get(field_name)}</td></tr>`
}

fn display(document, context) {
    let rows = "";
    rows += field_row(document, "message_type", "Message Type");
    rows += field_row(document, "processing_code", "Processing Code");
    rows += field_row(document, "amount", "Amount");
    rows += field_row(document, "transmission_time", "Transmission Time");
    rows += field_row(document, "stan", "STAN");
    rows += field_row(document, "terminal_id", "Terminal ID");
    rows += field_row(document, "currency", "Currency");

    `<section class="protocol-document">
        <h3>ISO 8583 Message</h3>
        <table><tbody>${rows}</tbody></table>
    </section>`
}
