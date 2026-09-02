#[derive(Debug)]
struct SequencedHttpCapabilities {
    inner: crate::http::PlainHttpCapabilityFactory,
    events: Arc<StdMutex<Vec<&'static str>>>,
}

impl SequencedHttpCapabilities {
    fn new(events: Arc<StdMutex<Vec<&'static str>>>) -> Self {
        Self {
            inner: crate::http::PlainHttpCapabilityFactory::new(
                "forward-test-workspace",
                "capability-sequence",
            ),
            events,
        }
    }

    fn direction<D: intercept_proxy_exchange::Direction>(
        &self,
        capabilities: crate::http::HttpDirectionCapabilities<D>,
        events: [&'static str; 4],
    ) -> crate::http::HttpDirectionCapabilities<D> {
        crate::http::HttpDirectionCapabilities::new(
            Box::new(SequencedDecode {
                inner: capabilities.decode,
                events: Arc::clone(&self.events),
                event: events[0],
            }),
            Box::new(SequencedDisplay {
                inner: capabilities.display,
                events: Arc::clone(&self.events),
                event: events[1],
            }),
            Box::new(SequencedRules {
                inner: capabilities.rules,
                events: Arc::clone(&self.events),
                event: events[2],
            }),
            Box::new(SequencedEncode {
                inner: capabilities.encode,
                events: Arc::clone(&self.events),
                event: events[3],
            }),
        )
    }
}

impl crate::http::HttpProtocolCapabilityFactory for SequencedHttpCapabilities {
    fn observation_metadata(&self) -> crate::http::HttpObservationMetadata {
        self.inner.observation_metadata()
    }

    fn create_upstream(
        &self,
        connection: crate::http::HttpConnectionIdentity,
    ) -> std::result::Result<
        crate::http::HttpDirectionCapabilities<intercept_proxy_exchange::Upstream>,
        intercept_proxy_exchange::Error,
    > {
        let capabilities = self.inner.create_upstream(connection)?;
        Ok(self.direction(
            capabilities,
            [
                "upstream.decode",
                "upstream.display",
                "upstream.rules",
                "upstream.encode",
            ],
        ))
    }

    fn create_downstream(
        &self,
        connection: crate::http::HttpConnectionIdentity,
    ) -> std::result::Result<
        crate::http::HttpDirectionCapabilities<intercept_proxy_exchange::Downstream>,
        intercept_proxy_exchange::Error,
    > {
        let capabilities = self.inner.create_downstream(connection)?;
        Ok(self.direction(
            capabilities,
            [
                "downstream.decode",
                "downstream.display",
                "downstream.rules",
                "downstream.encode",
            ],
        ))
    }
}

struct SequencedDecode<D: intercept_proxy_exchange::Direction> {
    inner: Box<dyn intercept_proxy_exchange::Decode<intercept_proxy_exchange::Http, D>>,
    events: Arc<StdMutex<Vec<&'static str>>>,
    event: &'static str,
}

#[async_trait::async_trait]
impl<D: intercept_proxy_exchange::Direction>
    intercept_proxy_exchange::Decode<intercept_proxy_exchange::Http, D> for SequencedDecode<D>
{
    async fn decode(
        &mut self,
        context: &intercept_proxy_exchange::HttpContext,
    ) -> std::result::Result<intercept_proxy_exchange::Document, intercept_proxy_exchange::Error>
    {
        self.events.lock().unwrap().push(self.event);
        self.inner.decode(context).await
    }
}

struct SequencedDisplay {
    inner: Box<dyn intercept_proxy_exchange::Display>,
    events: Arc<StdMutex<Vec<&'static str>>>,
    event: &'static str,
}

#[async_trait::async_trait]
impl intercept_proxy_exchange::Display for SequencedDisplay {
    async fn display(
        &mut self,
        document: &intercept_proxy_exchange::Document,
    ) -> std::result::Result<String, intercept_proxy_exchange::Error> {
        self.events.lock().unwrap().push(self.event);
        self.inner.display(document).await
    }
}

struct SequencedRules {
    inner: Box<dyn intercept_proxy_exchange::Rules>,
    events: Arc<StdMutex<Vec<&'static str>>>,
    event: &'static str,
}

#[async_trait::async_trait]
impl intercept_proxy_exchange::Rules for SequencedRules {
    async fn apply(
        &mut self,
        document: intercept_proxy_exchange::Document,
    ) -> std::result::Result<intercept_proxy_exchange::Document, intercept_proxy_exchange::Error>
    {
        self.events.lock().unwrap().push(self.event);
        self.inner.apply(document).await
    }
}

struct SequencedEncode<D: intercept_proxy_exchange::Direction> {
    inner: Box<dyn intercept_proxy_exchange::Encode<intercept_proxy_exchange::Http, D>>,
    events: Arc<StdMutex<Vec<&'static str>>>,
    event: &'static str,
}

#[async_trait::async_trait]
impl<D: intercept_proxy_exchange::Direction>
    intercept_proxy_exchange::Encode<intercept_proxy_exchange::Http, D> for SequencedEncode<D>
{
    async fn encode(
        &mut self,
        original: &intercept_proxy_exchange::HttpContext,
        document: &intercept_proxy_exchange::Document,
    ) -> std::result::Result<intercept_proxy_exchange::HttpContext, intercept_proxy_exchange::Error>
    {
        self.events.lock().unwrap().push(self.event);
        self.inner.encode(original, document).await
    }
}

#[tokio::test]
async fn real_http_exchange_runs_capabilities_in_pipeline_order() {
    let events = Arc::new(StdMutex::new(Vec::new()));
    let origin_events = Arc::clone(&events);
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin_address = origin.local_addr().unwrap();
    let origin_task = tokio::spawn(async move {
        let (mut stream, _) = origin.accept().await.unwrap();
        assert_eq!(read_raw_http_request_body(&mut stream).await, "request");
        origin_events.lock().unwrap().push("server.received");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nresponse")
            .await
            .unwrap();
    });

    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication))
        .unwrap()
        .with_pipeline(
            ChannelId::new("capability-sequence").unwrap(),
            Uuid::new_v4(),
            Arc::new(CapturingPipelinePorts::default()),
            Arc::new(SequencedHttpCapabilities::new(Arc::clone(&events))),
            MessageLimits::default(),
        );
    let (client, proxy) = tokio::io::duplex(16 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45039".parse().unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
    let connection_task = tokio::spawn(connection);
    let response = sender
        .send_request(
            Request::builder()
                .method(Method::POST)
                .uri(format!("http://{origin_address}/sequence"))
                .body(Full::new(Bytes::from_static(b"request")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "response"
    );
    events.lock().unwrap().push("app.received");

    drop(sender);
    connection_task.await.unwrap().unwrap();
    proxy_task.await.unwrap().unwrap();
    origin_task.await.unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        [
            "upstream.decode",
            "upstream.display",
            "upstream.rules",
            "upstream.encode",
            "server.received",
            "downstream.decode",
            "downstream.display",
            "downstream.rules",
            "downstream.encode",
            "app.received",
        ]
    );
}
