//! `intercept_proxy::exchange::ui` 结构化事件到连接级内存仓储的 tracing Layer。
//!
//! Layer 只读取 tracing 的 primitive fields。`record_debug` 被刻意忽略，避免把
//! `Debug`/fmt 文本误当成可逆协议数据。缺少 opened 元数据或事件字段时 fail-open 丢弃。

use std::{io, sync::Arc};

use chrono::Utc;
use intercept_proxy_application::{
    EventHub, ExchangeContext, ExchangeObservationEvent, ExchangeObservationRecord,
    ExchangeProtocol, ExternalPackageCallDiagnosticViewModel, ExternalPackageCallStage,
    UiEventPayload,
};
use intercept_proxy_domain::{
    ProtocolDirection, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
use intercept_proxy_infrastructure::{ExchangeObservationCounters, ExchangeObservationStore};
use tracing::{Event, Subscriber, span::Attributes};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

use super::nonblocking_queue::{
    BoundedMessage, BoundedSender, ConsumerOwner, QueueByteBudget, QueueDropReason,
    spawn_bounded_consumer,
};

const UI_TARGET: &str = "intercept_proxy::exchange::ui";

mod fields;

use fields::Fields;

#[derive(Clone, Debug)]
pub(crate) struct ExchangeUiLayer {
    sender: BoundedSender<QueuedObservation>,
    loss_sender: BoundedSender<LossSignal>,
    counters: Arc<ExchangeObservationCounters>,
    max_fields_bytes: usize,
}

#[derive(Debug)]
pub(crate) struct ExchangeUiConsumer {
    observations: ConsumerOwner<QueuedObservation>,
    losses: ConsumerOwner<LossSignal>,
}

impl ExchangeUiConsumer {
    pub(crate) fn shutdown(&self) -> io::Result<()> {
        let observation_result = self.observations.shutdown();
        let loss_result = self.losses.shutdown();
        observation_result.and(loss_result)
    }
}

impl ExchangeUiLayer {
    /// Starts one independent consumer for the already-validated UI event capacity.
    pub(crate) fn new(
        store: Arc<ExchangeObservationStore>,
        events: Arc<EventHub>,
        channel_capacity: usize,
        queue_budget: Arc<QueueByteBudget>,
    ) -> io::Result<(Self, ExchangeUiConsumer)> {
        let counters = store.counters();
        let max_fields_bytes = queue_budget.limit().saturating_sub(64).max(1);
        // Loss notification has an independent one-slot control lane. If it is full, the already
        // queued marker is guaranteed to publish; runtime-log payload pressure cannot consume it.
        let loss_events = Arc::clone(&events);
        let (loss_sender, loss_consumer) = spawn_bounded_consumer(
            "exchange-observation-loss",
            1,
            Arc::new(QueueByteBudget::new(64)),
            move |loss: LossSignal| publish_changed(&loss_events, None, None, loss.observed_at),
        )?;
        let (sender, consumer) = spawn_bounded_consumer(
            "exchange-observation",
            channel_capacity,
            queue_budget,
            move |fields| {
                record(&store, &events, &fields);
            },
        )?;
        Ok((
            Self {
                sender,
                loss_sender,
                counters,
                max_fields_bytes,
            },
            ExchangeUiConsumer {
                observations: consumer,
                losses: loss_consumer,
            },
        ))
    }

    #[cfg(test)]
    fn from_sender_for_test(
        sender: std::sync::mpsc::SyncSender<super::nonblocking_queue::Budgeted<QueuedObservation>>,
        counters: Arc<ExchangeObservationCounters>,
        queue_budget: Arc<QueueByteBudget>,
    ) -> (
        Self,
        std::sync::mpsc::Receiver<super::nonblocking_queue::Budgeted<LossSignal>>,
    ) {
        let max_fields_bytes = queue_budget.limit().saturating_sub(64).max(1);
        let (loss_sender, loss_receiver) = std::sync::mpsc::sync_channel(1);
        (
            Self {
                sender: BoundedSender::from_sync_sender(sender, queue_budget),
                loss_sender: BoundedSender::from_sync_sender(
                    loss_sender,
                    Arc::new(QueueByteBudget::new(64)),
                ),
                counters,
                max_fields_bytes,
            },
            loss_receiver,
        )
    }
}

#[derive(Clone, Debug)]
struct QueuedObservation {
    fields: Fields,
    observed_at: chrono::DateTime<Utc>,
}

#[derive(Clone, Debug)]
struct LossSignal {
    observed_at: chrono::DateTime<Utc>,
}

impl BoundedMessage for QueuedObservation {
    fn logical_bytes(&self) -> usize {
        self.fields.logical_bytes().saturating_add(64)
    }
}

impl BoundedMessage for LossSignal {
    fn logical_bytes(&self) -> usize {
        64
    }
}

impl<S> Layer<S> for ExchangeUiLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &tracing::span::Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut fields = Fields::new(self.max_fields_bytes);
        attrs.record(&mut fields);
        span.extensions_mut().insert(fields);
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        ctx: Context<'_, S>,
    ) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut update = Fields::new(self.max_fields_bytes);
        values.record(&mut update);
        let mut extensions = span.extensions_mut();
        if let Some(fields) = extensions.get_mut::<Fields>() {
            fields.merge(&update);
        } else {
            extensions.insert(update);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if event.metadata().target() != UI_TARGET {
            return;
        }
        let mut fields = Fields::new(self.max_fields_bytes);
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                if let Some(span_fields) = span.extensions().get::<Fields>() {
                    fields.merge(span_fields);
                }
            }
        }
        let mut event_fields = Fields::new(self.max_fields_bytes);
        event.record(&mut event_fields);
        fields.merge(&event_fields);
        let observed_at = Utc::now();
        if fields.overflowed() {
            self.note_loss(observed_at);
            return;
        }
        if let Err(
            QueueDropReason::Full
            | QueueDropReason::BytesFull
            | QueueDropReason::Disconnected
            | QueueDropReason::Contended,
        ) = self.sender.try_send(QueuedObservation {
            fields,
            observed_at,
        }) {
            self.note_loss(observed_at);
        }
    }
}

impl ExchangeUiLayer {
    fn note_loss(&self, observed_at: chrono::DateTime<Utc>) {
        self.counters.note_dropped();
        // This independent control lane is bounded to one coalesced marker. A full lane already
        // contains a marker that will publish the required refresh.
        let _ = self.loss_sender.try_send(LossSignal { observed_at });
    }
}

fn record(store: &ExchangeObservationStore, events: &EventHub, queued: &QueuedObservation) {
    let fields = &queued.fields;
    let observed_at = &queued.observed_at;
    let Some(event) = fields.text("event") else {
        note_ignored(store, events, fields, *observed_at);
        return;
    };
    let Some(exchange_id) = fields.text("exchange_id") else {
        note_ignored(store, events, fields, *observed_at);
        return;
    };
    if event == "opened" {
        if let Some(record) = opened_record(fields, exchange_id, *observed_at) {
            let runtime_epoch = record.runtime_epoch;
            let exchange_id = record.exchange_id.clone();
            if store.open(record) {
                publish_changed(events, Some(runtime_epoch), Some(exchange_id), *observed_at);
            } else {
                publish_ignored(fields, events, *observed_at);
            }
        } else {
            note_ignored(store, events, fields, *observed_at);
        }
        return;
    }
    let Some(event) = observation_event(fields, &event, *observed_at) else {
        note_ignored(store, events, fields, *observed_at);
        return;
    };
    let Some(protocol) = protocol(fields) else {
        note_ignored(store, events, fields, *observed_at);
        return;
    };
    let observed_runtime_epoch = match fields.text("runtime_epoch") {
        Some(value) => {
            let Some(value) = parse_id(value) else {
                note_ignored(store, events, fields, *observed_at);
                return;
            };
            Some(value)
        }
        None => None,
    };
    if let Some(runtime_epoch) = store.append(&exchange_id, protocol, observed_runtime_epoch, event)
    {
        publish_changed(events, Some(runtime_epoch), Some(exchange_id), *observed_at);
    } else {
        publish_ignored(fields, events, *observed_at);
    }
}

fn note_ignored(
    store: &ExchangeObservationStore,
    events: &EventHub,
    fields: &Fields,
    observed_at: chrono::DateTime<Utc>,
) {
    store.note_ignored_event();
    publish_ignored(fields, events, observed_at);
}

fn publish_ignored(fields: &Fields, events: &EventHub, observed_at: chrono::DateTime<Utc>) {
    let runtime_epoch = fields.text("runtime_epoch").and_then(parse_id);
    publish_changed(
        events,
        runtime_epoch,
        fields.text("exchange_id"),
        observed_at,
    );
}

fn publish_changed(
    events: &EventHub,
    runtime_epoch: Option<intercept_proxy_application::RuntimeEpoch>,
    exchange_id: Option<String>,
    observed_at: chrono::DateTime<Utc>,
) {
    // 该调用运行在独立观测消费者线程中；WebView 变慢或断开不会阻塞交易数据面。
    events.publish(
        runtime_epoch,
        observed_at,
        exchange_id,
        None,
        UiEventPayload::ExchangeObservationChanged,
    );
}

fn opened_record(
    fields: &Fields,
    exchange_id: String,
    observed_at: chrono::DateTime<Utc>,
) -> Option<ExchangeObservationRecord> {
    let workspace_id = parse_id(fields.text("workspace_id")?)?;
    let listener_id = parse_id(fields.text("listener_id")?)?;
    let runtime_epoch = parse_id(fields.text("runtime_epoch")?)?;
    let peer_address = fields.text("peer")?;
    let protocol = protocol(fields)?;
    Some(ExchangeObservationRecord {
        exchange_id,
        workspace_id,
        listener_id,
        runtime_epoch,
        peer_address,
        protocol,
        events: vec![ExchangeObservationEvent::Opened { observed_at }],
        evidence_evicted: false,
    })
}

fn parse_id<T: serde::de::DeserializeOwned>(value: String) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(value)).ok()
}

fn observation_event(
    fields: &Fields,
    event: &str,
    observed_at: chrono::DateTime<Utc>,
) -> Option<ExchangeObservationEvent> {
    match event {
        "received" => Some(ExchangeObservationEvent::Received {
            observed_at,
            direction: direction(fields)?,
            context: context(fields)?,
            document: optional_json(fields.text("document_json")).ok()?,
            display: fields.text("display"),
        }),
        "sent" => Some(ExchangeObservationEvent::Sent {
            observed_at,
            direction: direction(fields)?,
            context: context(fields)?,
        }),
        "failed" => Some(ExchangeObservationEvent::Failed {
            observed_at,
            direction: direction(fields),
            stage: fields.text("stage")?,
            context: context(fields),
            error: fields.text("error")?,
            external_package_call: external_package_call(fields),
        }),
        "closed" => Some(ExchangeObservationEvent::Closed {
            observed_at,
            outcome: fields.text("outcome")?,
            error: fields.text("error"),
        }),
        _ => None,
    }
}

fn external_package_call(fields: &Fields) -> Option<ExternalPackageCallDiagnosticViewModel> {
    let id = fields.text("external_package_id")?;
    if id.is_empty() {
        return None;
    }
    let version = fields.text("external_package_version")?;
    let stage = match fields.text("external_stage")?.as_str() {
        "frame" => ExternalPackageCallStage::Frame,
        "decode" => ExternalPackageCallStage::Decode,
        "display" => ExternalPackageCallStage::Display,
        "encode" => ExternalPackageCallStage::Encode,
        _ => return None,
    };
    let optional = |name: &str| fields.text(name).filter(|value| !value.is_empty());
    Some(ExternalPackageCallDiagnosticViewModel {
        package: ProtocolPackageRef {
            id: ProtocolPackageId::new(id).ok()?,
            version: ProtocolPackageVersion::new(version).ok()?,
        },
        direction: direction(fields)?,
        stage,
        method: fields.text("external_method")?,
        request_id: optional("external_request_id"),
        remote_code: fields
            .text("external_remote_code")
            .and_then(|value| value.parse().ok())
            .filter(|value| *value != 0),
        stable_code: optional("external_stable_code"),
        remote_message: optional("external_remote_message"),
        remote_data_summary: optional("external_remote_data_summary"),
    })
}

fn protocol(fields: &Fields) -> Option<ExchangeProtocol> {
    match fields.text("protocol")?.to_ascii_lowercase().as_str() {
        "http" => Some(ExchangeProtocol::Http),
        "socket" => Some(ExchangeProtocol::Socket),
        _ => None,
    }
}

fn optional_json(encoded: Option<String>) -> Result<Option<serde_json::Value>, serde_json::Error> {
    let Some(encoded) = encoded else {
        return Ok(None);
    };
    if encoded.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&encoded).map(Some)
}

fn direction(fields: &Fields) -> Option<ProtocolDirection> {
    match fields.text("direction")?.to_ascii_lowercase().as_str() {
        "upstream" => Some(ProtocolDirection::Upstream),
        "downstream" => Some(ProtocolDirection::Downstream),
        _ => None,
    }
}

fn context(fields: &Fields) -> Option<ExchangeContext> {
    match protocol(fields)? {
        ExchangeProtocol::Http => Some(ExchangeContext::Http {
            header: fields.text("context_header")?,
            body: fields.text("context_body")?,
            body_is_utf8: fields.text("context_body_is_utf8")?.parse().ok()?,
        }),
        ExchangeProtocol::Socket => Some(ExchangeContext::Socket {
            bytes: decode_hex(&fields.text("context_bytes_hex")?)?,
        }),
    }
}

fn decode_hex(encoded: &str) -> Option<Vec<u8>> {
    if !encoded.len().is_multiple_of(2) {
        return None;
    }
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(text, 16).ok()
        })
        .collect()
}

#[cfg(test)]
#[path = "exchange_ui_layer/tests.rs"]
mod tests;
