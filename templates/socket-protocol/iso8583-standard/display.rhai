// 可选入口：只负责把 Document 生成用于应用内嵌展示的 HTML。
// 没有此入口、Listener 关闭显示能力或脚本执行失败时，应用显示 Frame Hex。
// 返回值始终是不可信 HTML；协议运行时不会把它标记为安全内容。
// 应用必须在隔离页面中使用严格 CSP 展示，不能直接插入主页面 DOM。

import "libraries/iso8583" as iso8583;

fn field_row(document, name, label) {
    if !document.has(name) { return ""; }
    `<tr><th>${label}</th><td>${document.get(name)}</td></tr>`
}

fn display(document, context) {
    let rows = field_row(document, "message_type", "MTI");
    for spec in iso8583::field_specs() {
        rows += field_row(document, spec.name, "DE" + spec.number + " " + spec.name);
    }

    `<section class="protocol-document">
        <h3>ISO 8583:1987 Message</h3>
        <table><tbody>${rows}</tbody></table>
    </section>`
}
