//! Tauri Command 的薄适配层与 TypeScript 绑定清单。
//!
//! 各命令域位于独立子模块；这里仅公开稳定的 Rust IPC 接口并集中维护注册顺序。
//! 子模块只做参数/错误映射，业务规则、数据库和网络 I/O 仍由 application 层负责。

mod android;
mod app;
mod application_backup;
mod capture;
mod certificates;
mod diagnostics;
mod listener;
mod mcp;
mod protocol_packages;
mod protocol_rules;
mod rules;
mod settings;
mod workspace;

#[cfg(test)]
#[path = "e2e_tests/mod.rs"]
mod e2e_tests;

pub use android::*;
pub use app::*;
pub use application_backup::*;
pub use capture::*;
pub use certificates::*;
pub use diagnostics::*;
pub use listener::*;
pub use mcp::*;
pub use protocol_packages::*;
pub use protocol_rules::*;
pub use rules::*;
pub use settings::*;
pub use workspace::*;

use intercept_proxy_application::{AppError, AppErrorViewModel};
use tauri::Wry;
use tauri_specta::{Builder, collect_commands};

pub(super) type CommandResult<T> = Result<T, AppErrorViewModel>;

pub(super) fn command_error(error: AppError) -> AppErrorViewModel {
    error.into()
}

// 命令注册表刻意集中，便于逐项核对公开 IPC 面；这里不承载业务分支。
#[allow(clippy::too_many_lines)]
pub fn builder() -> Builder<Wry> {
    // Specta 默认把 Rust u64 映射为 JS BigInt，但静态导出的 WebView DTO 使用 number。
    // 这里允许转换的前提不是“任意 u64 都安全”，而是公开字段仅包含 revision、event cursor
    // 和容量计数；它们受仓储递增策略与产品容量上限约束，始终小于 Number.MAX_SAFE_INTEGER。
    // 新增无界 u64 字段时必须先改为字符串/BigInt DTO，不能沿用此全局转换假设。
    Builder::<Wry>::new()
        .dangerously_cast_bigints_to_number()
        .commands(collect_commands![
            app_bootstrap,
            app_subscribe_events,
            app_unsubscribe_events,
            mcp_info,
            diagnostic_log_query,
            diagnostic_reproduction_report_export,
            android_adb_get,
            android_adb_select,
            android_device_list,
            android_package_list,
            android_package_refresh,
            android_package_query,
            android_package_get,
            android_companion_install,
            android_companion_update,
            android_vpn_open_consent,
            device_network_profile_list,
            device_network_profile_new,
            device_network_profile_get,
            device_network_profile_apply_intent,
            device_network_profile_save,
            device_network_profile_delete,
            device_network_start,
            device_network_apply,
            device_network_stop,
            device_network_emergency_restore,
            device_network_status,
            device_network_endpoints,
            device_network_runtime_owner,
            workspace_list,
            workspace_get,
            workspace_secret_store_basic,
            workspace_create,
            workspace_copy,
            workspace_select,
            workspace_validate,
            workspace_save,
            workspace_delete,
            application_backup_export,
            application_backup_import_prepare,
            application_backup_import_commit,
            application_backup_import_discard,
            listener_list,
            listener_new,
            listener_copy,
            listener_get,
            listener_validate,
            listener_save,
            listener_delete,
            listener_statuses,
            listener_overview,
            listener_start,
            listener_stop,
            listener_test_upstream_connection,
            listener_test_upstream_tls,
            protocol_package_list,
            external_package_service_status,
            listener_protocol_package_catalog,
            protocol_package_detail,
            protocol_package_import,
            protocol_package_import_commit,
            protocol_package_import_discard,
            protocol_package_restore_builtin,
            protocol_package_export_builtin,
            protocol_package_enable,
            protocol_package_disable,
            protocol_package_delete,
            protocol_package_usage,
            listener_import_downstream_server_identity,
            listener_import_downstream_client_trust,
            listener_import_upstream_client_identity,
            listener_import_upstream_server_trust,
            listener_certificate_overview,
            listener_certificate_discard,
            capture_query,
            capture_get_detail,
            capture_clear_view,
            socket_capture_query,
            socket_capture_get_detail,
            socket_capture_clear,
            breakpoint_query,
            breakpoint_get,
            breakpoint_format_json,
            breakpoint_restore_original,
            breakpoint_validate,
            breakpoint_resolve,
            rule_list,
            rule_get,
            rule_new_draft,
            rule_condition_draft,
            rule_action_draft,
            rule_match_field_draft,
            rule_match_operator_draft,
            rule_parse_byte_input,
            rule_parse_header_input,
            rule_create_from_session,
            rule_save,
            rule_copy,
            rule_delete,
            rule_toggle,
            rule_import,
            rule_export,
            protocol_rule_list,
            protocol_rule_capabilities,
            protocol_rule_parse_value,
            protocol_rule_save,
            protocol_rule_toggle,
            protocol_rule_delete,
            fault_template_list,
            fault_configure,
            fault_active_list,
            fault_stop,
            certificate_overview,
            certificate_generate_ca,
            certificate_export_ca,
            certificate_reissue_leaf,
            certificate_import_pkcs12,
            certificate_import_upstream_ca,
            certificate_validate,
            certificate_reset_ca,
            settings_get,
            settings_validate,
            settings_save,
            settings_reset_defaults,
            application_data_reset,
        ])
}
