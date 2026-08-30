//! 跨命令、持久化和真实 IPC 的发布前集成证据。

mod support;

use intercept_proxy_application::{
    DiagnosticReportQuery, ProxyWorkspace, WorkspaceSummaryViewModel,
};
use serde_json::json;

use self::support::CrossLayerFixture;
use super::DiagnosticReportExportOutcome;

#[test]
fn diagnostic_report_export_uses_native_dialog_and_writes_copyable_markdown() {
    let fixture = CrossLayerFixture::new();
    let webview = fixture.webview();
    let selected = selected_workspace(&fixture, &webview);
    let workspace: ProxyWorkspace = fixture.invoke_ok(
        &webview,
        "workspace_get",
        json!({ "workspaceId": selected.id }),
    );
    let listener = workspace.listeners.first().expect("default listener");

    let outcome: Option<DiagnosticReportExportOutcome> = fixture.invoke_ok(
        &webview,
        "diagnostic_reproduction_report_export",
        json!({
            "query": DiagnosticReportQuery {
                workspace_id: workspace.id,
                listener_id: listener.id,
            }
        }),
    );

    assert!(outcome.expect("saved report").bytes_written > 0);
    let markdown = fixture.saved_report();
    assert!(markdown.contains("# Intercept Proxy 故障复现报告"));
    assert!(markdown.contains(&workspace.id.to_string()));
    assert!(markdown.contains(&listener.id.to_string()));
    assert!(markdown.contains("## 持久化应用运行日志"));
    fixture.assert_report_dialog_boundary();
}

#[test]
fn mcp_reproduction_report_and_runtime_logs_are_read_only_and_queryable() {
    let fixture = CrossLayerFixture::new();
    let webview = fixture.webview();
    let selected = selected_workspace(&fixture, &webview);
    let before: ProxyWorkspace = fixture.invoke_ok(
        &webview,
        "workspace_get",
        json!({ "workspaceId": selected.id }),
    );
    let listener = before.listeners.first().expect("default listener");

    let logs = fixture.call_mcp_tool("application_log_query", json!({ "limit": 10 }));
    let diagnostics = fixture.call_mcp_tool("diagnostics_query", json!({ "limit": 10 }));
    let exchanges = fixture.call_mcp_tool(
        "exchange_observation_query",
        json!({
            "workspace_id": before.id,
            "page": { "page": 1, "page_size": 10 },
        }),
    );
    let captures = fixture.call_mcp_tool("http_capture_query", json!({ "page_size": 10 }));
    let report = fixture.call_mcp_tool(
        "reproduction_report",
        json!({
            "workspace_id": before.id,
            "listener_id": listener.id,
        }),
    );
    let after: ProxyWorkspace = fixture.invoke_ok(
        &webview,
        "workspace_get",
        json!({ "workspaceId": selected.id }),
    );

    assert!(logs["rows"].is_array());
    assert!(logs["queue_dropped_full"].is_u64());
    assert!(logs["queue_dropped_disconnected"].is_u64());
    assert!(logs["queue_dropped_contended"].is_u64());
    for field in [
        "rows",
        "current_cursor",
        "oldest_retained_event_id",
        "snapshot_required",
        "retained_count",
        "truncated",
    ] {
        assert!(
            diagnostics.get(field).is_some(),
            "diagnostics missing {field}"
        );
    }
    for field in [
        "rows",
        "page",
        "page_size",
        "total",
        "evicted_records",
        "dropped_events",
        "ignored_events",
    ] {
        assert!(exchanges.get(field).is_some(), "exchanges missing {field}");
    }
    for field in [
        "rows",
        "total",
        "page",
        "page_size",
        "event_cursor",
        "oldest_event_id",
        "snapshot_required",
    ] {
        assert!(captures.get(field).is_some(), "captures missing {field}");
    }
    assert!(report["bundle"].is_object());
    assert!(report["application_logs"].is_object());
    assert!(report["application_logs"]["queue_dropped_full"].is_u64());
    assert!(report["markdown"].is_string());
    assert!(
        report["markdown"]
            .as_str()
            .is_some_and(|markdown| markdown.contains("持久化应用运行日志"))
    );
    assert_eq!(before, after, "read-only MCP must not mutate the workspace");
}

#[test]
fn javascript_package_import_reaches_phase9_enabled_failed_state_through_real_ipc() {
    let fixture = CrossLayerFixture::new();
    let webview = fixture.webview();

    let preview: serde_json::Value =
        fixture.invoke_ok(&webview, "protocol_package_import", json!({}));
    let committed: serde_json::Value = fixture.invoke_ok(
        &webview,
        "protocol_package_import_commit",
        json!({ "token": preview["token"] }),
    );
    let packages: serde_json::Value =
        fixture.invoke_ok(&webview, "protocol_package_list", json!({}));

    assert_eq!(preview["disposition"], "new");
    assert_eq!(committed["outcome"], "installed");
    assert_eq!(committed["version"]["enabled"], true);
    assert_eq!(committed["version"]["package_source"]["online"], false);
    assert_eq!(packages.as_array().unwrap().len(), 1);
    fixture.assert_dialog_boundaries();
}

fn selected_workspace(
    fixture: &CrossLayerFixture,
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
) -> WorkspaceSummaryViewModel {
    fixture
        .invoke_ok::<Vec<WorkspaceSummaryViewModel>>(webview, "workspace_list", json!({}))
        .into_iter()
        .find(|workspace| workspace.selected)
        .expect("default selected workspace")
}
