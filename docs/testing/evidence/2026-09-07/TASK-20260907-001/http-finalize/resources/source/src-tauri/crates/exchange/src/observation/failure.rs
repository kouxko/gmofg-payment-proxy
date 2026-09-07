//! Failure fields and their shared Context/external-call projection budget.

use super::{MAX_OBSERVATION_TEXT_BYTES, direction, hex, observation_lost};
use crate::{Direction, Error, ObservedContext, ObservedProtocol};

pub(crate) fn failed<D: Direction>(stage: &'static str, error: &Error) {
    let stage = error.stage.map_or(stage, crate::ErrorStage::as_str);
    if !fits_failure_projection(&[], error) {
        observation_lost("failure_projection_too_large");
        return;
    }
    tracing::error!(
        target: "intercept_proxy::exchange::ui",
        event = "failed",
        direction = direction::<D>(),
        stage,
        error = error.message.as_str(),
        external_package_id = error.external_package_call.as_ref().map_or("", |value| value.package.id.as_str()),
        external_package_version = error.external_package_call.as_ref().map_or("", |value| value.package.version.as_str()),
        external_stage = error.external_package_call.as_ref().map_or("", |value| external_stage(value.stage)),
        external_method = error.external_package_call.as_ref().map_or("", |value| value.method.as_str()),
        external_request_id = error.external_package_call.as_ref().and_then(|value| value.request_id.as_deref()).unwrap_or(""),
        external_remote_code = error.external_package_call.as_ref().and_then(|value| value.remote_code).unwrap_or(0),
        external_stable_code = error.external_package_call.as_ref().and_then(|value| value.stable_code.as_deref()).unwrap_or(""),
        external_remote_message = error.external_package_call.as_ref().and_then(|value| value.remote_message.as_deref()).unwrap_or(""),
        external_remote_data_summary = error.external_package_call.as_ref().and_then(|value| value.remote_data_summary.as_deref()).unwrap_or(""),
    );
}

pub(crate) fn failed_with_context<P, D>(stage: &'static str, context: &P::Context, error: &Error)
where
    P: ObservedProtocol,
    D: Direction,
{
    let stage = error.stage.map_or(stage, crate::ErrorStage::as_str);
    match P::observed(context) {
        ObservedContext::Http {
            header,
            body,
            body_is_utf8,
        } if fits_failure_projection(&[header, body], error) => {
            tracing::error!(
                target: "intercept_proxy::exchange::ui",
                event = "failed",
                protocol = P::NAME,
                direction = direction::<D>(),
                stage,
                context_header = header,
                context_body = body,
                context_body_is_utf8 = body_is_utf8,
                context_bytes_hex = tracing::field::Empty,
                error = error.message.as_str(),
                external_package_id = error.external_package_call.as_ref().map_or("", |value| value.package.id.as_str()),
                external_package_version = error.external_package_call.as_ref().map_or("", |value| value.package.version.as_str()),
                external_stage = error.external_package_call.as_ref().map_or("", |value| external_stage(value.stage)),
                external_method = error.external_package_call.as_ref().map_or("", |value| value.method.as_str()),
                external_request_id = error.external_package_call.as_ref().and_then(|value| value.request_id.as_deref()).unwrap_or(""),
                external_remote_code = error.external_package_call.as_ref().and_then(|value| value.remote_code).unwrap_or(0),
                external_stable_code = error.external_package_call.as_ref().and_then(|value| value.stable_code.as_deref()).unwrap_or(""),
                external_remote_message = error.external_package_call.as_ref().and_then(|value| value.remote_message.as_deref()).unwrap_or(""),
                external_remote_data_summary = error.external_package_call.as_ref().and_then(|value| value.remote_data_summary.as_deref()).unwrap_or(""),
            );
        }
        ObservedContext::Http { .. } => observation_lost("http_failure_projection_too_large"),
        ObservedContext::Socket { data } => match hex(data) {
            Some(encoded) if fits_failure_projection(&[&encoded], error) => {
                tracing::error!(
                    target: "intercept_proxy::exchange::ui",
                    event = "failed",
                    protocol = P::NAME,
                    direction = direction::<D>(),
                    stage,
                    context_header = tracing::field::Empty,
                    context_body = tracing::field::Empty,
                    context_bytes_hex = encoded.as_str(),
                    error = error.message.as_str(),
                    external_package_id = error.external_package_call.as_ref().map_or("", |value| value.package.id.as_str()),
                    external_package_version = error.external_package_call.as_ref().map_or("", |value| value.package.version.as_str()),
                    external_stage = error.external_package_call.as_ref().map_or("", |value| external_stage(value.stage)),
                    external_method = error.external_package_call.as_ref().map_or("", |value| value.method.as_str()),
                    external_request_id = error.external_package_call.as_ref().and_then(|value| value.request_id.as_deref()).unwrap_or(""),
                    external_remote_code = error.external_package_call.as_ref().and_then(|value| value.remote_code).unwrap_or(0),
                    external_stable_code = error.external_package_call.as_ref().and_then(|value| value.stable_code.as_deref()).unwrap_or(""),
                    external_remote_message = error.external_package_call.as_ref().and_then(|value| value.remote_message.as_deref()).unwrap_or(""),
                    external_remote_data_summary = error.external_package_call.as_ref().and_then(|value| value.remote_data_summary.as_deref()).unwrap_or(""),
                );
            }
            _ => observation_lost("socket_failure_projection_too_large"),
        },
    }
}

const fn external_stage(stage: crate::ExternalPackageCallStage) -> &'static str {
    match stage {
        crate::ExternalPackageCallStage::Frame => "frame",
        crate::ExternalPackageCallStage::Decode => "decode",
        crate::ExternalPackageCallStage::Display => "display",
        crate::ExternalPackageCallStage::Encode => "encode",
        crate::ExternalPackageCallStage::Finalize => "finalize",
    }
}

fn fits_failure_projection(context: &[&str], error: &Error) -> bool {
    let mut bytes = context.iter().try_fold(error.message.len(), |sum, value| {
        sum.checked_add(value.len())
    });
    if let Some(external) = error.external_package_call.as_deref() {
        for value in [
            external.package.id.as_str(),
            external.package.version.as_str(),
            external_stage(external.stage),
            external.method.as_str(),
            external.request_id.as_deref().unwrap_or(""),
            external.stable_code.as_deref().unwrap_or(""),
            external.remote_message.as_deref().unwrap_or(""),
            external.remote_data_summary.as_deref().unwrap_or(""),
        ] {
            bytes = bytes.and_then(|sum| sum.checked_add(value.len()));
        }
    }
    bytes.is_some_and(|total| total <= MAX_OBSERVATION_TEXT_BYTES)
}
