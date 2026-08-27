//! Exchange UI tracing 的稳定字段投影。
//!
//! UI Layer 只读取这些 primitive 字段，不解析 `Debug` 文本。序列化属于观测旁路；超限
//! 或失败时只发出固定大小的 loss 事件，绝不改变业务 `Result`。

use crate::{Direction, Envelope, Error, ObservedContext, ObservedProtocol};

/// A single UI tracing projection may never allocate or copy more than this fixed text budget.
/// The authoritative network payload remains owned by the business pipeline; overflow only drops
/// observation evidence and is surfaced through the runtime loss counter.
const MAX_OBSERVATION_TEXT_BYTES: usize = 16 * 1024 * 1024;

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
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing::{Event, Subscriber, field::Visit, subscriber::with_default};
    use tracing_subscriber::{Layer, layer::Context, prelude::*, registry::LookupSpan};

    use super::{MAX_OBSERVATION_TEXT_BYTES, raw_failed, raw_received};
    use crate::{Error, Upstream};

    #[derive(Default)]
    struct CapturedFields {
        event: Option<String>,
        context_bytes: Option<String>,
    }

    impl Visit for CapturedFields {
        fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {}

        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            match field.name() {
                "event" => self.event = Some(value.to_owned()),
                "context_bytes_hex" => self.context_bytes = Some(value.to_owned()),
                _ => {}
            }
        }
    }

    struct CaptureLayer(Arc<Mutex<CapturedFields>>);

    impl<S> Layer<S> for CaptureLayer
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
            let mut fields = CapturedFields::default();
            event.record(&mut fields);
            *self.0.lock().unwrap() = fields;
        }
    }

    #[test]
    fn raw_failure_without_bytes_omits_context_instead_of_forging_empty_frame() {
        let captured = Arc::new(Mutex::new(CapturedFields::default()));
        let subscriber = tracing_subscriber::registry().with(CaptureLayer(Arc::clone(&captured)));

        with_default(subscriber, || {
            raw_failed::<Upstream>("read", None, &Error::new("peer closed"));
        });

        let captured = captured.lock().unwrap();
        assert_eq!(captured.event.as_deref(), Some("failed"));
        assert!(captured.context_bytes.is_none());
    }

    #[test]
    fn oversized_raw_payload_emits_fixed_loss_without_building_hex_projection() {
        let captured = Arc::new(Mutex::new(CapturedFields::default()));
        let subscriber = tracing_subscriber::registry().with(CaptureLayer(Arc::clone(&captured)));
        let bytes = vec![0xA5; MAX_OBSERVATION_TEXT_BYTES / 2 + 1];

        with_default(subscriber, || raw_received::<Upstream>(&bytes));

        let captured = captured.lock().unwrap();
        assert_eq!(captured.event.as_deref(), Some("observation_lost"));
        assert!(captured.context_bytes.is_none());
    }
}
