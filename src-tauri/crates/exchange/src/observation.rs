//! Exchange UI tracing 的稳定字段投影。
//!
//! UI Layer 只读取这些 primitive 字段，不解析 `Debug` 文本。序列化属于观测旁路；超限
//! 或失败时只发出固定大小的 loss 事件，绝不改变业务 `Result`。

use serde::Serialize;

use crate::{
    Direction, Document, Envelope, Error, ObservedContext, ObservedProtocol, ProtocolDirection,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuleProcessingChange {
    pub rule_id: String,
    pub matched: bool,
    pub operations: Vec<RuleProcessingOperation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleProcessingOperationKind {
    RecordMatch,
    Set,
    Clear,
    Insert,
    Append,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuleProcessingOperation {
    pub kind: RuleProcessingOperationKind,
    pub path: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct RuleProcessingAccumulator {
    changes: Vec<RuleProcessingChange>,
    retained_bytes: usize,
    truncated: bool,
}

impl RuleProcessingAccumulator {
    pub fn record(&mut self, change: RuleProcessingChange) {
        if self.truncated {
            return;
        }
        let Ok(bytes) = serde_json::to_vec(&change) else {
            self.truncated = true;
            return;
        };
        let next = self.retained_bytes.saturating_add(bytes.len());
        if next > MAX_OBSERVATION_TEXT_BYTES {
            self.truncated = true;
            return;
        }
        self.retained_bytes = next;
        self.changes.push(change);
    }

    #[must_use]
    pub fn changes(&self) -> &[RuleProcessingChange] {
        &self.changes
    }

    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

pub fn rules_processed(
    direction: ProtocolDirection,
    changes: &RuleProcessingAccumulator,
    final_document: &Document,
) {
    let Some(final_document_json) = document_json(final_document) else {
        observation_lost("final_document_too_large_or_unserializable");
        return;
    };
    let changes_budget = MAX_OBSERVATION_TEXT_BYTES.saturating_sub(final_document_json.len());
    let Some((changes_json, serialization_truncated)) =
        serialize_changes(changes.changes(), changes_budget)
    else {
        observation_lost("rule_changes_unserializable");
        return;
    };
    tracing::info!(
        target: "intercept_proxy::exchange::ui",
        event = "processed",
        direction = direction_name(direction),
        changes_json = changes_json.as_str(),
        changes_truncated = changes.truncated() || serialization_truncated,
        final_document_json = final_document_json.as_str(),
    );
}

pub(crate) fn encoded<P, D>(context: &P::Context)
where
    P: ObservedProtocol,
    D: Direction,
{
    observe_context::<P, D>("encoded", context);
}

/// A single UI tracing projection may never allocate or copy more than this fixed text budget.
/// The authoritative network payload remains owned by the business pipeline; overflow only drops
/// observation evidence and is surfaced through the runtime loss counter.
pub const MAX_OBSERVATION_TEXT_BYTES: usize = 16 * 1024 * 1024;

fn serialize_changes(changes: &[RuleProcessingChange], budget: usize) -> Option<(String, bool)> {
    if budget < 2 {
        return Some(("[]".to_owned(), !changes.is_empty()));
    }
    let mut output = String::with_capacity(budget.min(4096));
    output.push('[');
    for (index, change) in changes.iter().enumerate() {
        let item = serde_json::to_string(change).ok()?;
        let separator = usize::from(index > 0);
        if output
            .len()
            .saturating_add(separator)
            .saturating_add(item.len())
            .saturating_add(1)
            > budget
        {
            output.push(']');
            return Some((output, true));
        }
        if index > 0 {
            output.push(',');
        }
        output.push_str(&item);
    }
    output.push(']');
    Some((output, false))
}

pub(crate) fn received<P, D>(envelope: &Envelope<P, D>)
where
    P: ObservedProtocol,
    D: Direction,
{
    let Some(document_json) = document_json(envelope.document()) else {
        observation_lost("document_too_large_or_unserializable");
        return;
    };
    match P::observed(envelope.context()) {
        ObservedContext::Http {
            header,
            body,
            body_is_utf8,
        } => {
            if !fits_projection(&[header, body, &document_json, envelope.display()]) {
                observation_lost("http_received_projection_too_large");
                return;
            }
            tracing::info!(
                target: "intercept_proxy::exchange::ui",
                event = "received",
                protocol = P::NAME,
                direction = direction::<D>(),
                context_header = header,
                context_body = body,
                context_body_is_utf8 = body_is_utf8,
                context_bytes_hex = tracing::field::Empty,
                document_json = document_json.as_str(),
                display = envelope.display(),
            );
        }
        ObservedContext::Socket { data } => {
            let Some(encoded) = hex(data) else {
                observation_lost("socket_received_projection_too_large");
                return;
            };
            if !fits_projection(&[&encoded, &document_json, envelope.display()]) {
                observation_lost("socket_received_projection_too_large");
                return;
            }
            tracing::info!(
                target: "intercept_proxy::exchange::ui",
                event = "received",
                protocol = P::NAME,
                direction = direction::<D>(),
                context_header = tracing::field::Empty,
                context_body = tracing::field::Empty,
                context_bytes_hex = encoded.as_str(),
                document_json = document_json.as_str(),
                display = envelope.display(),
            );
        }
    }
}

pub(crate) fn sent<P, D>(context: &P::Context)
where
    P: ObservedProtocol,
    D: Direction,
{
    match P::observed(context) {
        ObservedContext::Http {
            header,
            body,
            body_is_utf8,
        } if fits_projection(&[header, body]) => {
            tracing::info!(
                target: "intercept_proxy::exchange::ui",
                event = "sent",
                protocol = P::NAME,
                direction = direction::<D>(),
                context_header = header,
                context_body = body,
                context_body_is_utf8 = body_is_utf8,
                context_bytes_hex = tracing::field::Empty,
            );
        }
        ObservedContext::Http { .. } => observation_lost("http_sent_projection_too_large"),
        ObservedContext::Socket { data } => match hex(data) {
            Some(encoded) => tracing::info!(
                target: "intercept_proxy::exchange::ui",
                event = "sent",
                protocol = P::NAME,
                direction = direction::<D>(),
                context_header = tracing::field::Empty,
                context_body = tracing::field::Empty,
                context_bytes_hex = encoded.as_str(),
            ),
            None => observation_lost("socket_sent_projection_too_large"),
        },
    }
}

fn observe_context<P, D>(event: &'static str, context: &P::Context)
where
    P: ObservedProtocol,
    D: Direction,
{
    match P::observed(context) {
        ObservedContext::Http {
            header,
            body,
            body_is_utf8,
        } if fits_projection(&[header, body]) => tracing::info!(
            target: "intercept_proxy::exchange::ui", event, protocol = P::NAME,
            direction = direction::<D>(), context_header = header, context_body = body,
            context_body_is_utf8 = body_is_utf8, context_bytes_hex = tracing::field::Empty,
        ),
        ObservedContext::Http { .. } => observation_lost("http_encoded_projection_too_large"),
        ObservedContext::Socket { data } => match hex(data) {
            Some(encoded) => tracing::info!(
                target: "intercept_proxy::exchange::ui", event, protocol = P::NAME,
                direction = direction::<D>(), context_header = tracing::field::Empty,
                context_body = tracing::field::Empty, context_bytes_hex = encoded.as_str(),
            ),
            None => observation_lost("socket_encoded_projection_too_large"),
        },
    }
}

pub(crate) fn failed<D: Direction>(stage: &'static str, error: &Error) {
    if !fits_projection(&[error.message.as_str()]) {
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
    match P::observed(context) {
        ObservedContext::Http {
            header,
            body,
            body_is_utf8,
        } if fits_projection(&[header, body, error.message.as_str()]) => {
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
            Some(encoded) if fits_projection(&[&encoded, error.message.as_str()]) => {
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
    }
}

pub(crate) fn raw_received<D: Direction>(bytes: &[u8]) {
    let Some(encoded) = hex(bytes) else {
        observation_lost("raw_received_projection_too_large");
        return;
    };
    tracing::info!(
        target: "intercept_proxy::exchange::ui",
        event = "received",
        protocol = "socket",
        direction = direction::<D>(),
        context_bytes_hex = encoded.as_str(),
    );
}

pub(crate) fn raw_sent<D: Direction>(bytes: &[u8]) {
    let Some(encoded) = hex(bytes) else {
        observation_lost("raw_sent_projection_too_large");
        return;
    };
    tracing::info!(
        target: "intercept_proxy::exchange::ui",
        event = "sent",
        protocol = "socket",
        direction = direction::<D>(),
        context_bytes_hex = encoded.as_str(),
    );
}

pub(crate) fn raw_failed<D: Direction>(stage: &'static str, bytes: Option<&[u8]>, error: &Error) {
    if let Some(bytes) = bytes {
        let Some(encoded) = hex(bytes) else {
            observation_lost("raw_failure_projection_too_large");
            return;
        };
        if !fits_projection(&[&encoded, error.message.as_str()]) {
            observation_lost("raw_failure_projection_too_large");
            return;
        }
        tracing::error!(
            target: "intercept_proxy::exchange::ui",
            event = "failed",
            protocol = "socket",
            direction = direction::<D>(),
            stage,
            context_bytes_hex = encoded.as_str(),
            error = error.message.as_str(),
        );
    } else {
        tracing::error!(
            target: "intercept_proxy::exchange::ui",
            event = "failed",
            protocol = "socket",
            direction = direction::<D>(),
            stage,
            context_bytes_hex = tracing::field::Empty,
            error = error.message.as_str(),
        );
    }
}

fn direction<D: Direction>() -> &'static str {
    match D::KIND {
        crate::DirectionKind::Upstream => "upstream",
        crate::DirectionKind::Downstream => "downstream",
    }
}

fn direction_name(direction: ProtocolDirection) -> &'static str {
    match direction {
        ProtocolDirection::Upstream => "upstream",
        ProtocolDirection::Downstream => "downstream",
    }
}

fn document_json(document: &crate::Document) -> Option<String> {
    let mut output = LimitedBytes::default();
    match serde_json::to_writer(&mut output, document) {
        Ok(()) => String::from_utf8(output.bytes).ok(),
        Err(error) => {
            tracing::warn!(
                target: "intercept_proxy::exchange::diagnostic",
                error = %error,
                "Document observation serialization failed"
            );
            None
        }
    }
}

fn hex(bytes: &[u8]) -> Option<String> {
    use std::fmt::Write as _;

    if bytes.len().saturating_mul(2) > MAX_OBSERVATION_TEXT_BYTES {
        return None;
    }
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(encoded, "{byte:02X}");
    }
    Some(encoded)
}

fn fits_projection(values: &[&str]) -> bool {
    values
        .iter()
        .try_fold(0_usize, |total, value| total.checked_add(value.len()))
        .is_some_and(|total| total <= MAX_OBSERVATION_TEXT_BYTES)
}

fn observation_lost(reason: &'static str) {
    tracing::warn!(
        target: "intercept_proxy::exchange::ui",
        event = "observation_lost",
        reason,
    );
}

#[derive(Default)]
struct LimitedBytes {
    bytes: Vec<u8>,
}

impl std::io::Write for LimitedBytes {
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(input.len()) > MAX_OBSERVATION_TEXT_BYTES {
            return Err(std::io::Error::other(
                "observation JSON exceeds fixed projection limit",
            ));
        }
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "observation/tests.rs"]
mod tests;
