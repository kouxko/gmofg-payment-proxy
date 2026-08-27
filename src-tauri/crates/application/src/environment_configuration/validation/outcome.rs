use std::time::Duration;

use super::{
    EnvironmentStatusCode, EnvironmentValidationLayer, EnvironmentValidationReport,
    EnvironmentValidationResult, EnvironmentValidationStatus, ORDER,
    status_code::status_code_from_error,
};
use crate::AppResult;

pub(super) enum LayerOutcome<T = EnvironmentValidationStatus> {
    Completed(AppResult<T>),
    Cancelled,
    TotalDeadline,
    LayerDeadline,
}

pub(super) fn layer_failure(
    error: &crate::AppError,
    mut layers: Vec<EnvironmentValidationResult>,
    layer: EnvironmentValidationLayer,
    duration_ms: u64,
) -> EnvironmentValidationReport {
    let code = status_code_from_error(error, layer);
    let reason = stable_failure_reason(layer);
    layers.push(EnvironmentValidationResult {
        layer,
        status: EnvironmentValidationStatus::Failed,
        code: Some(code),
        reason: Some(reason),
        duration_ms,
    });
    append_skipped(&mut layers, layer, "dependency_not_satisfied");
    EnvironmentValidationReport {
        layers,
        status_code: Some(code),
    }
}

pub(super) fn cancelled_report(
    mut layers: Vec<EnvironmentValidationResult>,
    layer: EnvironmentValidationLayer,
    duration_ms: u64,
    reason: &'static str,
    code: EnvironmentStatusCode,
) -> EnvironmentValidationReport {
    layers.push(cancelled(layer, duration_ms, reason));
    append_skipped(&mut layers, layer, reason);
    EnvironmentValidationReport {
        layers,
        status_code: Some(code),
    }
}

const fn stable_failure_reason(layer: EnvironmentValidationLayer) -> &'static str {
    match layer {
        EnvironmentValidationLayer::DnsTcpPort => "dns_tcp_port_validation_failed",
        EnvironmentValidationLayer::TlsMtls => "tls_mtls_validation_failed",
        _ => "dependency_validation_failed",
    }
}

pub(super) fn cancelled(
    layer: EnvironmentValidationLayer,
    duration_ms: u64,
    reason: &'static str,
) -> EnvironmentValidationResult {
    EnvironmentValidationResult {
        layer,
        status: EnvironmentValidationStatus::Cancelled,
        code: None,
        reason: Some(reason),
        duration_ms,
    }
}

pub(super) fn append_skipped(
    results: &mut Vec<EnvironmentValidationResult>,
    current: EnvironmentValidationLayer,
    reason: &'static str,
) {
    let current_index = ORDER
        .iter()
        .position(|layer| *layer == current)
        .expect("known validation layer");
    results.extend(ORDER[current_index + 1..].iter().copied().map(|layer| {
        EnvironmentValidationResult {
            layer,
            status: EnvironmentValidationStatus::SkippedDependency,
            code: None,
            reason: Some(reason),
            duration_ms: 0,
        }
    }));
}

pub(super) fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
