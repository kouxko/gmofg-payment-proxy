use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, OnceLock},
};

use async_trait::async_trait;
use parking_lot::Mutex;
use tracing::{Event, Id, Subscriber, field::Visit, span::Attributes};
use tracing_subscriber::{Layer, layer::Context, prelude::*, registry::LookupSpan};

use super::{
    super::*,
    support::{document, text},
};

type EventLog = Arc<Mutex<Vec<String>>>;

struct ExchangeHarness {
    exchange: Exchange<Http>,
    events: EventLog,
    responses: Arc<Mutex<Vec<HttpContext>>>,
    connect_count: Arc<Mutex<usize>>,
}

struct AppReader {
    requests: VecDeque<String>,
    events: EventLog,
}

#[async_trait]
impl Reader<Http, Upstream> for AppReader {
    async fn read(&mut self) -> Result<Option<HttpContext>, Error> {
        let Some(body) = self.requests.pop_front() else {
            self.events.lock().push("app_eof".to_owned());
            return Ok(None);
        };
        self.events.lock().push(format!("app_read:{body}"));
        Ok(Some(HttpContext {
            header: "POST /sale HTTP/1.1".to_owned(),
            body,
            body_is_utf8: true,
        }))
    }
}

struct AppWriter {
    events: EventLog,
    responses: Arc<Mutex<Vec<HttpContext>>>,
}

#[async_trait]
impl Writer<Http, Downstream> for AppWriter {
    async fn write(&mut self, context: HttpContext) -> Result<HttpContext, Error> {
        self.events
            .lock()
            .push(format!("app_write:{}", context.body));
        self.responses.lock().push(context.clone());
        Ok(context)
    }
}

struct AppConnectionImpl {
    reader: AppReader,
    writer: AppWriter,
    events: EventLog,
    shutdown_error: Option<Error>,
}

#[async_trait]
impl Connection<Http, Upstream, Downstream> for AppConnectionImpl {
    fn reader(&mut self) -> &mut dyn Reader<Http, Upstream> {
        &mut self.reader
    }

    fn writer(&mut self) -> &mut dyn Writer<Http, Downstream> {
        &mut self.writer
    }

    async fn shutdown(&mut self) -> Result<(), Error> {
        self.events.lock().push("app_shutdown".to_owned());
        self.shutdown_error.clone().map_or(Ok(()), Err)
    }
}

struct EchoServer {
    events: EventLog,
    connect_count: Arc<Mutex<usize>>,
    reply: bool,
    shutdown_error: Option<Error>,
}

#[async_trait]
impl Server<Http> for EchoServer {
    async fn connect(
        &mut self,
        _first: &HttpContext,
    ) -> Result<Box<ServerConnection<Http>>, Error> {
        *self.connect_count.lock() += 1;
        self.events.lock().push("server_connect".to_owned());
        let responses = Arc::new(Mutex::new(VecDeque::new()));
        Ok(Box::new(EchoServerConnection {
            reader: EchoServerReader {
                events: Arc::clone(&self.events),
                responses: Arc::clone(&responses),
                reply: self.reply,
            },
            writer: EchoServerWriter {
                events: Arc::clone(&self.events),
                responses,
            },
            events: Arc::clone(&self.events),
            shutdown_error: self.shutdown_error.clone(),
        }))
    }
}

struct EchoServerReader {
    events: EventLog,
    responses: Arc<Mutex<VecDeque<HttpContext>>>,
    reply: bool,
}

#[async_trait]
impl Reader<Http, Downstream> for EchoServerReader {
    async fn read(&mut self) -> Result<Option<HttpContext>, Error> {
        self.events.lock().push("server_read".to_owned());
        if self.reply {
            Ok(self.responses.lock().pop_front())
        } else {
            Ok(None)
        }
    }
}

struct EchoServerWriter {
    events: EventLog,
    responses: Arc<Mutex<VecDeque<HttpContext>>>,
}

#[async_trait]
impl Writer<Http, Upstream> for EchoServerWriter {
    async fn write(&mut self, context: HttpContext) -> Result<HttpContext, Error> {
        self.events
            .lock()
            .push(format!("server_write:{}", context.body));
        self.responses.lock().push_back(context.clone());
        Ok(context)
    }
}

struct EchoServerConnection {
    reader: EchoServerReader,
    writer: EchoServerWriter,
    events: EventLog,
    shutdown_error: Option<Error>,
}

#[async_trait]
impl Connection<Http, Downstream, Upstream> for EchoServerConnection {
    fn reader(&mut self) -> &mut dyn Reader<Http, Downstream> {
        &mut self.reader
    }

    fn writer(&mut self) -> &mut dyn Writer<Http, Upstream> {
        &mut self.writer
    }

    async fn shutdown(&mut self) -> Result<(), Error> {
        self.events.lock().push("server_shutdown".to_owned());
        self.shutdown_error.clone().map_or(Ok(()), Err)
    }
}

struct TextCodec;

#[async_trait]
impl<D: Direction> Decode<Http, D> for TextCodec {
    async fn decode(&mut self, context: &HttpContext) -> Result<Document, Error> {
        Ok(document(&context.body))
    }
}

#[async_trait]
impl<D: Direction> Encode<Http, D> for TextCodec {
    async fn encode(
        &mut self,
        original: &HttpContext,
        document: &Document,
    ) -> Result<HttpContext, Error> {
        Ok(HttpContext {
            header: original.header.clone(),
            body: text(document),
            body_is_utf8: true,
        })
    }
}

struct TextDisplay;

#[async_trait]
impl Display for TextDisplay {
    async fn display(&mut self, document: &Document) -> Result<String, Error> {
        Ok(text(document))
    }
}

struct IdentityRules;

#[async_trait]
impl Rules for IdentityRules {
    async fn apply(&mut self, document: Document) -> Result<Document, Error> {
        Ok(document)
    }
}

fn pipeline<D: Direction>() -> Pipeline<Http, D> {
    Pipeline::new(
        Box::new(HttpRead::new(Box::new(TextCodec), Box::new(TextDisplay))),
        Box::new(Write::new(Box::new(IdentityRules), Box::new(TextCodec))),
    )
}

fn exchange(
    requests: &[&str],
    reply: bool,
    app_shutdown_error: Option<Error>,
    server_shutdown_error: Option<Error>,
) -> ExchangeHarness {
    let events = Arc::new(Mutex::new(Vec::new()));
    let responses = Arc::new(Mutex::new(Vec::new()));
    let connect_count = Arc::new(Mutex::new(0));
    let app = AppConnectionImpl {
        reader: AppReader {
            requests: requests.iter().map(|value| (*value).to_owned()).collect(),
            events: Arc::clone(&events),
        },
        writer: AppWriter {
            events: Arc::clone(&events),
            responses: Arc::clone(&responses),
        },
        events: Arc::clone(&events),
        shutdown_error: app_shutdown_error,
    };
    let server = EchoServer {
        events: Arc::clone(&events),
        connect_count: Arc::clone(&connect_count),
        reply,
        shutdown_error: server_shutdown_error,
    };
    let protocol = ProtocolExchange::new(
        Box::new(app),
        ServerSlot::new(Box::new(server)),
        pipeline::<Upstream>(),
        pipeline::<Downstream>(),
    );
    ExchangeHarness {
        exchange: Exchange::protocol(ExchangeId::new(7), protocol),
        events,
        responses,
        connect_count,
    }
}

#[tokio::test]
async fn exchange_is_strictly_paired_and_reuses_one_fixed_endpoint() {
    let harness = exchange(&["one", "two"], true, None, None);

    harness.exchange.exchange().await.unwrap();

    assert_eq!(*harness.connect_count.lock(), 1);
    assert_eq!(
        harness
            .responses
            .lock()
            .iter()
            .map(|response| response.body.as_str())
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
    assert_eq!(
        &*harness.events.lock(),
        &[
            "app_read:one",
            "server_connect",
            "server_write:one",
            "server_read",
            "app_write:one",
            "app_read:two",
            "server_write:two",
            "server_read",
            "app_write:two",
            "app_eof",
            "app_shutdown",
            "server_shutdown",
        ]
    );
}

#[tokio::test]
async fn server_eof_before_the_required_response_fails_the_exchange() {
    let harness = exchange(&["one"], false, None, None);

    let error = harness.exchange.exchange().await.unwrap_err();

    assert_eq!(error.message, "Server disconnected before replying");
    assert_eq!(harness.events.lock().last().unwrap(), "server_shutdown");
}

#[tokio::test]
async fn final_shutdown_failures_are_diagnostic_and_do_not_override_success() {
    let harness = exchange(
        &["one"],
        true,
        Some(Error::new("app close failed")),
        Some(Error::new("server close failed")),
    );

    harness.exchange.exchange().await.unwrap();
}

#[tokio::test]
async fn local_protocol_server_echoes_the_exact_socket_context() {
    let expected = SocketContext {
        data: vec![0, 1, 0xff],
    };
    let mut slot = ServerSlot::new(Box::new(LocalSocketServer::new()));
    Writer::<Socket, Upstream>::write(&mut slot, expected.clone())
        .await
        .unwrap();

    let echoed = slot.reader().unwrap().read().await.unwrap().unwrap();

    assert_eq!(echoed, expected);
}

#[test]
fn exchange_id_is_an_internal_correlation_value() {
    assert_eq!(ExchangeId::new(42).value(), 42);
}

#[derive(Default)]
struct EventFields {
    event: Option<String>,
    stage: Option<String>,
    exchange_id: Option<String>,
}

impl Visit for EventFields {
    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {}

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "event" => self.event = Some(value.to_owned()),
            "stage" => self.stage = Some(value.to_owned()),
            "exchange_id" => self.exchange_id = Some(value.to_owned()),
            _ => {}
        }
    }
}

#[derive(Clone)]
struct EventLayer(ObservationEvents);

#[derive(Clone)]
struct ObservedExchangeId(String);

type ObservationEvents = Arc<Mutex<HashMap<String, Vec<(String, Option<String>)>>>>;

static OBSERVATION_EVENTS: OnceLock<ObservationEvents> = OnceLock::new();

fn observation_events() -> &'static ObservationEvents {
    OBSERVATION_EVENTS.get_or_init(|| {
        // tracing callsite interest is cached process-wide. A thread-local subscriber can
        // therefore miss a callsite first registered concurrently on a test thread without a
        // subscriber. Keep one process-wide test subscriber and isolate timelines by ExchangeId.
        let events = Arc::new(Mutex::new(HashMap::new()));
        let subscriber = tracing_subscriber::registry().with(EventLayer(Arc::clone(&events)));
        tracing::subscriber::set_global_default(subscriber)
            .expect("exchange tests install the only process-wide tracing subscriber");
        events
    })
}

fn clear_observation_events(id: ExchangeId) {
    observation_events().lock().remove(&id.trace_value());
}

fn take_observation_events(id: ExchangeId) -> Vec<(String, Option<String>)> {
    observation_events()
        .lock()
        .remove(&id.trace_value())
        .unwrap_or_default()
}

impl<S> Layer<S> for EventLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        let mut fields = EventFields::default();
        attributes.record(&mut fields);
        let Some(exchange_id) = fields.exchange_id else {
            return;
        };
        if let Some(span) = context.span(id) {
            span.extensions_mut()
                .insert(ObservedExchangeId(exchange_id));
        }
    }

    fn on_event(&self, event: &Event<'_>, context: Context<'_, S>) {
        if event.metadata().target() != "intercept_proxy::exchange::ui" {
            return;
        }
        let Some(exchange_id) = context
            .event_span(event)
            .and_then(|span| span.extensions().get::<ObservedExchangeId>().cloned())
        else {
            return;
        };
        let mut fields = EventFields::default();
        event.record(&mut fields);
        if let Some(name) = fields.event {
            self.0
                .lock()
                .entry(exchange_id.0)
                .or_default()
                .push((name, fields.stage));
        }
    }
}

#[tokio::test]
async fn capability_factory_error_is_a_complete_exchange_timeline() {
    let id = ExchangeId::new(8);
    clear_observation_events(id);
    let result = Exchange::<Http>::protocol_with(id, || Err(Error::new("factory failed"))).await;

    assert_eq!(result.unwrap_err().message, "factory failed");
    assert_eq!(
        take_observation_events(id),
        &[
            ("opened".to_owned(), None),
            ("failed".to_owned(), Some("capability_factory".to_owned())),
            ("closed".to_owned(), None),
        ]
    );
}

#[tokio::test]
async fn capability_factory_panic_is_caught_in_the_same_exchange() {
    let id = ExchangeId::new(9);
    clear_observation_events(id);
    let result =
        Exchange::<Http>::protocol_with(id, || panic!("factory panic must not escape")).await;

    assert!(
        result
            .unwrap_err()
            .message
            .contains("CAPABILITY_FACTORY_PANICKED")
    );
    let events = take_observation_events(id);
    assert_eq!(events[0].0, "opened");
    assert_eq!(events[1].1.as_deref(), Some("capability_factory"));
    assert_eq!(events[2].0, "closed");
}
