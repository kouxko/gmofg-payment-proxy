//! 故障复现报告的纯 Markdown 组装逻辑。

use std::fmt::Write as _;

use crate::{
    DIAGNOSTIC_REPORT_MARKDOWN_MAX_CHARS, DIAGNOSTIC_REPORT_MAX_CAPTURES,
    DIAGNOSTIC_REPORT_MAX_DIAGNOSTICS, DiagnosticReportBundle, HttpBodyProcessing,
    ListenerDataPlane, SocketPayloadProcessing,
};

use super::bound_package;

pub(super) fn render_markdown(bundle: &DiagnosticReportBundle) -> String {
    let runtime = bundle
        .runtime_status
        .as_ref()
        .map_or("未知", |status| status.state_text.as_str());
    let package = bound_package(&bundle.listener).map_or_else(
        || "未绑定（透明/明文处理）".to_owned(),
        |package| format!("{}@{}", package.id.as_str(), package.version.as_str()),
    );
    let (data_plane, topology, processing, forwarding) = listener_shape(&bundle.listener);
    let mut markdown = format!(
        "# Intercept Proxy 故障复现报告\n\n生成时间：{}\n\n## 环境\n\n- 产品：{} {}\n- 系统：{} / {}\n- Workspace：{} (`{}`)\n- Listener：{} (`{}`)\n- 监听地址：{}:{}\n- 运行状态：{}\n- 协议包：{}\n- 诊断日志：{} 条（最多 {} 条）\n- Socket 抓包：{} 条（最多 {} 条）\n\n## 复现步骤\n\n",
        bundle.generated_at.to_rfc3339(),
        bundle.environment.product_name,
        bundle.environment.application_version,
        bundle.environment.operating_system,
        bundle.environment.architecture,
        bundle.workspace.name,
        bundle.workspace.id,
        bundle.listener.name,
        bundle.listener.id,
        bundle.listener.bind_address,
        bundle.listener.port,
        runtime,
        package,
        bundle.diagnostics.len(),
        DIAGNOSTIC_REPORT_MAX_DIAGNOSTICS,
        bundle.socket_captures.rows.len(),
        DIAGNOSTIC_REPORT_MAX_CAPTURES,
    );
    writeln!(markdown, "- 数据平面：{data_plane}").expect("writing to String cannot fail");
    writeln!(markdown, "- 网络拓扑：{topology}").expect("writing to String cannot fail");
    writeln!(markdown, "- 报文处理：{processing}").expect("writing to String cannot fail");
    writeln!(markdown, "- 转发方式：{forwarding}\n").expect("writing to String cannot fail");
    for (index, step) in bundle.reproduction_steps.iter().enumerate() {
        writeln!(markdown, "{}. {}", index + 1, step).expect("writing to String cannot fail");
    }
    markdown.push_str("\n## 最近诊断\n\n");
    for row in &bundle.diagnostics {
        writeln!(
            markdown,
            "- {} [{} / {}] {}\n",
            row.occurred_at.to_rfc3339(),
            row.level_text,
            row.stage_text,
            row.summary
        )
        .expect("writing to String cannot fail");
    }
    markdown.push_str("\n## Socket 抓包索引\n\n");
    for row in &bundle.socket_captures.rows {
        writeln!(
            markdown,
            "- `{}` {} 原始 {} B / 写出 {} B\n",
            row.capture_id,
            row.occurred_at.to_rfc3339(),
            row.origin_size_bytes,
            row.written_size_bytes
        )
        .expect("writing to String cannot fail");
    }
    if let Some(detail) = &bundle.capture_detail {
        let (input, written) = capture_wire_summary(detail);
        markdown.push_str("\n## 指定 Capture 测试数据\n\n");
        writeln!(markdown, "- 输入：{} B，Hex 预览 `{}`", input.0, input.1)
            .expect("writing to String cannot fail");
        writeln!(
            markdown,
            "- 写出：{} B，Hex 预览 `{}`",
            written.0, written.1
        )
        .expect("writing to String cannot fail");
    }
    if !bundle.collection_errors.is_empty() {
        markdown.push_str("\n## 采集缺口\n\n");
        for error in &bundle.collection_errors {
            writeln!(
                markdown,
                "- `{:?}` / `{}`：{}\n",
                error.section, error.code, error.message
            )
            .expect("writing to String cannot fail");
        }
    }
    markdown.push_str("\n## 架构引用\n\n");
    for reference in &bundle.environment.architecture_refs {
        writeln!(markdown, "- `{reference}`").expect("writing to String cannot fail");
    }
    markdown
}

fn listener_shape(listener: &crate::ProxyListener) -> (String, String, String, String) {
    match &listener.data_plane {
        ListenerDataPlane::Http(settings) => {
            let processing = match &settings.body_processing {
                HttpBodyProcessing::Plain => "plain".into(),
                HttpBodyProcessing::Protocol { package } => {
                    format!(
                        "protocol {}@{}",
                        package.id.as_str(),
                        package.version.as_str()
                    )
                }
            };
            let forwarding = settings.fixed_server.as_ref().map_or_else(
                || "按客户端请求目标动态转发".into(),
                |server| format!("固定 Server {}", server.upstream_url),
            );
            ("HTTP".into(), "HTTP proxy".into(), processing, forwarding)
        }
        ListenerDataPlane::Socket(settings) => {
            let (topology, forwarding) = match &settings.topology {
                crate::SocketTopology::Relay(relay) => (
                    "relay".into(),
                    format!(
                        "固定 Server {}:{}",
                        relay.upstream.host, relay.upstream.port
                    ),
                ),
                crate::SocketTopology::LocalResponder(_) => {
                    ("local_responder".into(), "本地生成应答（无上游）".into())
                }
            };
            let processing = match &settings.processing {
                SocketPayloadProcessing::Direct => "direct（透明字节转发）".into(),
                SocketPayloadProcessing::Scripted(scripted) => format!(
                    "scripted {}@{}",
                    scripted.package.id.as_str(),
                    scripted.package.version.as_str()
                ),
            };
            ("Socket".into(), topology, processing, forwarding)
        }
    }
}

fn capture_wire_summary(
    detail: &crate::SocketCaptureDetailViewModel,
) -> ((usize, String), (usize, String)) {
    use crate::SocketCapturePayload;

    let (input, written) = match &detail.record.payload {
        SocketCapturePayload::RelayFrame(capture) => (&capture.origin, &capture.written),
        SocketCapturePayload::LocalExchange(capture) => {
            (&capture.request_origin, &capture.written_response)
        }
        SocketCapturePayload::LocalExchangeFailure(capture) => {
            (&capture.request_origin, &capture.written_response_prefix)
        }
    };
    (
        (input.len(), hex_preview(input)),
        (written.len(), hex_preview(written)),
    )
}

fn hex_preview(bytes: &[u8]) -> String {
    const MAX_PREVIEW_BYTES: usize = 64;
    let mut preview = String::with_capacity(MAX_PREVIEW_BYTES * 2 + 1);
    for byte in bytes.iter().take(MAX_PREVIEW_BYTES) {
        write!(preview, "{byte:02x}").expect("writing to String cannot fail");
    }
    if bytes.len() > MAX_PREVIEW_BYTES {
        preview.push('…');
    }
    preview
}

pub(super) fn bounded_markdown(markdown: String) -> String {
    if markdown.chars().count() <= DIAGNOSTIC_REPORT_MARKDOWN_MAX_CHARS {
        return markdown;
    }
    markdown
        .chars()
        .take(DIAGNOSTIC_REPORT_MARKDOWN_MAX_CHARS.saturating_sub(20))
        .chain("\n\n[报告已截断]\n".chars())
        .collect()
}
