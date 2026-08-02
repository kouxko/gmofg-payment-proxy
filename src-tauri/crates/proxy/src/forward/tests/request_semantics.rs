#[test]
fn absolute_form_is_converted_to_origin_form() {
    let uri: Uri = "http://example.test:8080/path?q=1".parse().unwrap();
    assert_eq!(absolute_uri_to_origin_form(&uri).unwrap(), "/path?q=1");
    let root: Uri = "http://example.test".parse().unwrap();
    assert_eq!(absolute_uri_to_origin_form(&root).unwrap(), "/");
}

#[test]
fn strips_static_and_connection_declared_hop_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(
        CONNECTION,
        HeaderValue::from_static("keep-alive, x-private-hop"),
    );
    headers.insert("keep-alive", HeaderValue::from_static("timeout=5"));
    headers.insert("x-private-hop", HeaderValue::from_static("remove"));
    headers.insert(PROXY_AUTHORIZATION, HeaderValue::from_static("redacted"));
    headers.insert("x-end-to-end", HeaderValue::from_static("keep"));
    strip_hop_by_hop_headers(&mut headers);
    assert!(!headers.contains_key(CONNECTION));
    assert!(!headers.contains_key("x-private-hop"));
    assert!(!headers.contains_key(PROXY_AUTHORIZATION));
    assert_eq!(headers["x-end-to-end"], "keep");
}

#[tokio::test]
async fn pipeline_response_consumes_disconnect_schedule() {
    let message = Message::response(
        StatusCode::OK,
        &HeaderMap::new(),
        Bytes::from_static(b"abcd"),
    );
    let response = response_from_pipeline_disposition(
        ResponseDisposition::Send {
            message,
            schedule: TrafficSchedule {
                disconnect_after_bytes: Some(2),
                ..TrafficSchedule::default()
            },
        },
        &CancellationToken::new(),
    )
    .unwrap()
    .unwrap();
    let mut body = response.into_body();
    let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert_eq!(first, Bytes::from_static(b"ab"));
    assert!(body.frame().await.unwrap().is_err());
}
