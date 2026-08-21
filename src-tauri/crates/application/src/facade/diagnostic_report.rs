//! 故障复现报告的只读聚合用例。

use super::Application;
use crate::{
    AppError, AppResult, DIAGNOSTIC_REPORT_MAX_CAPTURES, DIAGNOSTIC_REPORT_MAX_DIAGNOSTICS,
    DiagnosticReportBundle, DiagnosticReportCollectionError, DiagnosticReportEnvironment,
    DiagnosticReportQuery, DiagnosticReportSection, DiagnosticReportViewModel, HttpBodyProcessing,
    ListenerDataPlane, ListenerRuntimeState, ListenerStatusViewModel, PageRequest,
    ProtocolPackageRef, SocketCapturePageViewModel, SocketCaptureQuery, SocketCaptureSort,
    SocketPayloadProcessing, SortDirection, UiTone,
};

mod markdown;

use markdown::{bounded_markdown, render_markdown};

impl Application {
    /// 聚合一个精确 Listener 的配置、运行态、日志、抓包和协议包现场。
    ///
    /// Workspace 或 Listener 身份错误会直接失败；其他只读观测失败会写入
    /// `collection_errors`，确保发生部分故障时仍能导出已有复现证据。
    pub async fn diagnostic_report_generate(
        &self,
        query: DiagnosticReportQuery,
    ) -> AppResult<DiagnosticReportViewModel> {
        let workspace = self.workspaces.get(query.workspace_id).await?;
        let listener = workspace
            .listeners
            .iter()
            .find(|listener| listener.id == query.listener_id)
            .cloned()
            .ok_or_else(|| {
                AppError::new(
                    "LISTENER_NOT_FOUND",
                    "指定 Listener 不属于请求的 Workspace，无法生成复现报告。",
                )
                .entity(query.workspace_id.to_string())
            })?;

        let mut collection_errors = Vec::new();
        let runtime_status = self
            .report_runtime_status(&listener, &mut collection_errors)
            .await;
        let settings = observe(
            self.settings.get().await,
            DiagnosticReportSection::Settings,
            &mut collection_errors,
        );
        let external_package_service = observe(
            self.external_packages.service_status().await,
            DiagnosticReportSection::ExternalPackageService,
            &mut collection_errors,
        );

        let package = bound_package(&listener);
        let protocol_package_detail = match package.as_ref() {
            Some(package) => observe(
                self.protocol_package_detail(package.clone()).await,
                DiagnosticReportSection::ProtocolPackageDetail,
                &mut collection_errors,
            ),
            None => None,
        };
        let protocol_rules = workspace
            .protocol_rules
            .iter()
            .filter(|rule| rule.listener_id() == listener.id)
            .cloned()
            .collect();

        let diagnostics = self.report_diagnostics(listener.id);
        let socket_captures = self
            .report_socket_captures(
                workspace.id,
                listener.id,
                package.clone(),
                &mut collection_errors,
            )
            .await;
        let capture_detail = self
            .report_capture_detail(
                query.capture_id,
                workspace.id,
                listener.id,
                &mut collection_errors,
            )
            .await;
        let android = self
            .report_android_observations(&mut collection_errors)
            .await;
        let environment = report_environment(&self.product_name);
        let reproduction_steps = reproduction_steps(&workspace, &listener, package.as_ref());
        let bundle = DiagnosticReportBundle {
            generated_at: chrono::Utc::now(),
            workspace,
            listener,
            runtime_status,
            settings,
            protocol_rules,
            protocol_package_detail,
            external_package_service,
            diagnostics,
            socket_captures,
            capture_detail,
            android_network_status: android.network_status,
            android_runtime_owner: android.runtime_owner,
            android_runtime_endpoints: android.runtime_endpoints,
            environment,
            reproduction_steps,
            collection_errors,
        };
        let markdown = bounded_markdown(render_markdown(&bundle));
        Ok(DiagnosticReportViewModel { bundle, markdown })
    }

    async fn report_runtime_status(
        &self,
        listener: &crate::ProxyListener,
        errors: &mut Vec<DiagnosticReportCollectionError>,
    ) -> Option<ListenerStatusViewModel> {
        match self.listener_runtime.statuses().await {
            Ok(statuses) => statuses
                .into_iter()
                .find(|status| status.listener_id == listener.id)
                .or_else(|| Some(stopped_status(listener))),
            Err(error) => {
                collect_error(errors, DiagnosticReportSection::RuntimeStatus, error);
                None
            }
        }
    }

    fn report_diagnostics(
        &self,
        listener_id: crate::ListenerId,
    ) -> Vec<crate::DiagnosticLogRowViewModel> {
        let listener_id = listener_id.to_string();
        self.events
            .diagnostic_log_snapshot()
            .into_iter()
            .filter(|row| row.listener_id.as_deref() == Some(listener_id.as_str()))
            .rev()
            .take(DIAGNOSTIC_REPORT_MAX_DIAGNOSTICS)
            .collect()
    }

    async fn report_socket_captures(
        &self,
        workspace_id: crate::WorkspaceId,
        listener_id: crate::ListenerId,
        package: Option<ProtocolPackageRef>,
        errors: &mut Vec<DiagnosticReportCollectionError>,
    ) -> SocketCapturePageViewModel {
        let query = SocketCaptureQuery {
            workspace_id: Some(workspace_id),
            listener_id: Some(listener_id),
            session_id: None,
            connection_id: None,
            package,
            direction: None,
            kind: None,
            occurred_from: None,
            occurred_to: None,
            sort: SocketCaptureSort::OccurredAt,
            direction_sort: SortDirection::Desc,
            page: PageRequest {
                page: 1,
                page_size: DIAGNOSTIC_REPORT_MAX_CAPTURES,
            },
        };
        match self.capture.query_socket(query).await {
            Ok(page) => page,
            Err(error) => {
                collect_error(errors, DiagnosticReportSection::SocketCaptures, error);
                empty_capture_page()
            }
        }
    }

    async fn report_capture_detail(
        &self,
        capture_id: Option<crate::SocketCaptureId>,
        workspace_id: crate::WorkspaceId,
        listener_id: crate::ListenerId,
        errors: &mut Vec<DiagnosticReportCollectionError>,
    ) -> Option<crate::SocketCaptureDetailViewModel> {
        let detail = match capture_id {
            Some(capture_id) => observe(
                self.capture.get_socket_detail(capture_id).await,
                DiagnosticReportSection::CaptureDetail,
                errors,
            ),
            None => None,
        }?;
        if detail.record.workspace_id == workspace_id && detail.record.listener_id == listener_id {
            Some(detail)
        } else {
            collect_error(
                errors,
                DiagnosticReportSection::CaptureDetail,
                AppError::new(
                    "CAPTURE_SCOPE_MISMATCH",
                    "指定 Socket capture 不属于请求的 Workspace 与 Listener。",
                ),
            );
            None
        }
    }

    async fn report_android_observations(
        &self,
        errors: &mut Vec<DiagnosticReportCollectionError>,
    ) -> ReportAndroidObservations {
        ReportAndroidObservations {
            network_status: observe(
                self.android.network_status().await,
                DiagnosticReportSection::AndroidNetworkStatus,
                errors,
            ),
            runtime_owner: observe_optional(
                self.android.runtime_owner().await,
                DiagnosticReportSection::AndroidRuntimeOwner,
                errors,
            ),
            runtime_endpoints: observe_vec(
                self.android.network_runtime_endpoints(None).await,
                DiagnosticReportSection::AndroidRuntimeEndpoints,
                errors,
            ),
        }
    }
}

struct ReportAndroidObservations {
    network_status: Option<crate::AndroidNetworkStatusViewModel>,
    runtime_owner: Option<crate::AndroidRuntimeOwnerViewModel>,
    runtime_endpoints: Vec<crate::AndroidRuntimeEndpointViewModel>,
}

fn report_environment(product_name: &str) -> DiagnosticReportEnvironment {
    DiagnosticReportEnvironment {
        product_name: product_name.into(),
        application_version: env!("CARGO_PKG_VERSION").into(),
        operating_system: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        architecture_refs: vec![
            "docs/architecture/system-context.md".into(),
            "docs/architecture/data-planes.md".into(),
            "docs/architecture/decisions/ADR-004-embedded-read-only-mcp.md".into(),
            "src-tauri/crates/application/src/facade/diagnostic_report.rs".into(),
            "src-tauri/crates/proxy/src/socket_relay".into(),
        ],
    }
}

fn bound_package(listener: &crate::ProxyListener) -> Option<ProtocolPackageRef> {
    match &listener.data_plane {
        ListenerDataPlane::Http(settings) => match &settings.body_processing {
            HttpBodyProcessing::Plain => None,
            HttpBodyProcessing::Protocol { package } => Some(package.clone()),
        },
        ListenerDataPlane::Socket(settings) => match &settings.processing {
            SocketPayloadProcessing::Direct => None,
            SocketPayloadProcessing::Scripted(scripted) => Some(scripted.package.clone()),
        },
    }
}

fn stopped_status(listener: &crate::ProxyListener) -> ListenerStatusViewModel {
    ListenerStatusViewModel {
        listener_id: listener.id,
        state: ListenerRuntimeState::Stopped,
        state_text: "已停止".into(),
        ui_tone: UiTone::Neutral,
        listen_address: format!("{}:{}", listener.bind_address, listener.port),
        fault_reason: None,
        can_start: true,
        can_stop: false,
        active_connections: 0,
        client_to_server_bytes: 0,
        server_to_client_bytes: 0,
        retained_diagnostic_evictions: 0,
    }
}

fn observe<T>(
    result: AppResult<T>,
    section: DiagnosticReportSection,
    errors: &mut Vec<DiagnosticReportCollectionError>,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            collect_error(errors, section, error);
            None
        }
    }
}

fn collect_error(
    errors: &mut Vec<DiagnosticReportCollectionError>,
    section: DiagnosticReportSection,
    error: AppError,
) {
    errors.push(DiagnosticReportCollectionError {
        section,
        code: error.view_model.code,
        message: error.view_model.message,
    });
}

fn observe_optional<T>(
    result: AppResult<Option<T>>,
    section: DiagnosticReportSection,
    errors: &mut Vec<DiagnosticReportCollectionError>,
) -> Option<T> {
    observe(result, section, errors).flatten()
}

fn observe_vec<T>(
    result: AppResult<Vec<T>>,
    section: DiagnosticReportSection,
    errors: &mut Vec<DiagnosticReportCollectionError>,
) -> Vec<T> {
    observe(result, section, errors).unwrap_or_default()
}

fn empty_capture_page() -> SocketCapturePageViewModel {
    SocketCapturePageViewModel {
        rows: Vec::new(),
        total: 0,
        page: 1,
        page_size: DIAGNOSTIC_REPORT_MAX_CAPTURES,
        total_pages: 0,
        empty_message: "Socket 抓包查询失败；请查看报告采集错误。".into(),
    }
}

fn reproduction_steps(
    workspace: &crate::ProxyWorkspace,
    listener: &crate::ProxyListener,
    package: Option<&ProtocolPackageRef>,
) -> Vec<String> {
    let mut steps = vec![
        format!(
            "导入或选择 Workspace `{}`（`{}`）。",
            workspace.name, workspace.id
        ),
        format!(
            "确认 Listener `{}`（`{}`）配置与报告一致。",
            listener.name, listener.id
        ),
    ];
    if let Some(package) = package {
        steps.push(format!(
            "连接并启用精确协议包 `{}@{}`，不要自动升级或回退版本。",
            package.id.as_str(),
            package.version.as_str()
        ));
    }
    steps.extend([
        format!("启动入口 `{}:{}`。", listener.bind_address, listener.port),
        "按 Socket 抓包中的方向、顺序和测试输入重放请求。".into(),
        "以报告中的诊断时间线和 capture ID 对比运行结果。".into(),
    ]);
    steps
}
