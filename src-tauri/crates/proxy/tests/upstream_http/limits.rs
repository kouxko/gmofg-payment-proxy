#[tokio::test]
async fn upstream_informational_heads_do_not_replace_the_final_raw_response_head() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).await.unwrap();
        stream
            .write_all(
                b"HTTP/1.1 100 Continue\r\nX-Info:\t first \r\n\r\n\
HTTP/1.1 103 Early Hints\r\nLink: </style.css>\r\n\r\n\
HTTP/1.1 207 Product Final\r\nX-Final:\t  yes \t\r\n\
Content-Length: 2\r\nConnection: close\r\n\r\nOK",
            )
            .await
            .unwrap();
    });
    let connector = fault_test_connector(address);

    let exchange = connector
        .send(
            &test_context(address),
            &NoopPipelinePorts,
            fault_test_request(),
            &[],
            None,
            &CancellationToken::new(),
        )
        .await
        .expect("final response");
    assert_eq!(
        exchange.informational_heads,
        vec![
            Bytes::from_static(b"HTTP/1.1 100 Continue\r\nX-Info:\t first \r\n\r\n"),
            Bytes::from_static(b"HTTP/1.1 103 Early Hints\r\nLink: </style.css>\r\n\r\n"),
        ]
    );
    let response = exchange.final_response;

    assert_eq!(response.start_line, "HTTP/1.1 207 Product Final");
    assert_eq!(response.http_status(), Some(207));
    assert_eq!(response.body, Bytes::from_static(b"OK"));
    assert!(
        response
            .reconstruct()
            .starts_with(b"HTTP/1.1 207 Product Final\r\nX-Final:\t  yes \t\r\n")
    );
    server.await.unwrap();
}

#[tokio::test]
async fn oversized_upstream_body_is_classified() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n12345")
            .await
            .unwrap();
    });
    let connector = HyperUpstreamConnector {
        address,
        host: "localhost".into(),
        host_header: "localhost".into(),
        rewrite_host: true,
        tls: None,
        connect_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        limits: MessageLimits {
            max_body_bytes: 4,
            ..MessageLimits::default()
        },
    };
    let request = ForwardRequest {
        method: Method::GET,
        uri: Uri::from_static("/"),
        message: Message::request(
            &Method::GET,
            &Uri::from_static("/"),
            &HeaderMap::new(),
            Bytes::new(),
        ),
    };
    let error = connector
        .send(
            &test_context(address),
            &NoopPipelinePorts,
            request,
            &[],
            None,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "BODY_TOO_LARGE");
    server.await.unwrap();
}

fn fault_test_connector(address: std::net::SocketAddr) -> HyperUpstreamConnector {
    HyperUpstreamConnector {
        address,
        host: "upstream.test".into(),
        host_header: "upstream.test".into(),
        rewrite_host: true,
        tls: None,
        connect_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(2),
        limits: MessageLimits::default(),
    }
}

fn fault_test_request() -> ForwardRequest {
    let mut headers = HeaderMap::new();
    headers.insert("host", HeaderValue::from_static("alpha-client"));
    headers.insert("content-length", HeaderValue::from_static("3"));
    ForwardRequest {
        method: Method::POST,
        uri: Uri::from_static("/resource"),
        message: Message::request(
            &Method::POST,
            &Uri::from_static("/resource"),
            &headers,
            Bytes::from_static(b"raw"),
        ),
    }
}

async fn accept_request_until_eof(listener: TcpListener) -> Vec<u8> {
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    stream.read_to_end(&mut request).await.unwrap();
    request
}
