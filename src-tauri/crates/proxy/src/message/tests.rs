use super::*;

#[test]
fn preserves_unmodified_raw_body_and_recalculates_modified_length() {
    let body = Bytes::from_static(&[0x81, 0x00, 0xff]);
    let mut message = Message {
        start_line: "POST / HTTP/1.1".into(),
        headers: vec![RawHeader::new(
            Bytes::from_static(b"Content-Length"),
            Bytes::from_static(b"999"),
        )],
        body: body.clone(),
        body_modified: false,
    };
    assert_eq!(message.passthrough_body(), body);
    message.replace_body(Bytes::from_static(b"OK"));
    assert_eq!(message.declared_content_length(), Some(2));
}

#[test]
fn setting_content_length_removes_transfer_encoding() {
    let mut message = Message::from_raw_http1_head(
        b"HTTP/1.1 404 Not Found\r\nTransfer-Encoding: chunked\r\n\r\n",
        Bytes::from_static(b"decoded body"),
    )
    .expect("buffered chunked response");

    message.set_content_length(message.body.len());

    assert_eq!(message.declared_content_length(), Some(12));
    assert!(
        !message
            .headers
            .iter()
            .any(|header| header.name.eq_ignore_ascii_case(b"transfer-encoding"))
    );
}

#[test]
fn replacing_chunked_body_preserves_transfer_encoding_without_content_length() {
    let mut message = Message::from_raw_http1_head(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
        Bytes::from_static(b"old"),
    )
    .expect("buffered chunked response");

    message.replace_body(Bytes::from_static(b"changed body"));

    assert!(message.uses_transfer_encoding());
    assert_eq!(message.declared_content_length(), None);
    assert_eq!(message.body, Bytes::from_static(b"changed body"));
}

#[test]
fn close_delimited_transfer_coding_is_extended_with_chunked() {
    let mut message = Message::from_raw_http1_head(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\n\r\n",
        Bytes::from_static(b"encoded"),
    )
    .expect("close-delimited transfer-coded response");

    message.ensure_transfer_encoding_ends_in_chunked();

    assert!(message.headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case(b"transfer-encoding")
            && header.value.eq_ignore_ascii_case(b"gzip, chunked")
    }));
}

#[test]
fn exposes_response_status_without_misclassifying_request_start_lines() {
    let response = Message::response(StatusCode::BAD_GATEWAY, &HeaderMap::new(), Bytes::new());
    let request = Message::request(
        &Method::POST,
        &"/resource".parse().expect("URI"),
        &HeaderMap::new(),
        Bytes::new(),
    );

    assert_eq!(response.http_status(), Some(502));
    assert_eq!(request.http_status(), None);
}

#[test]
fn reconstruction_uses_crlf_and_exact_body() {
    let message = Message {
        start_line: "HTTP/1.1 200 OK".into(),
        headers: vec![RawHeader::new(
            Bytes::from_static(b"x-test"),
            Bytes::from_static(b"yes"),
        )],
        body: Bytes::from_static(b"\0raw"),
        body_modified: false,
    };
    assert_eq!(
        &message.reconstruct()[..],
        b"HTTP/1.1 200 OK\r\nx-test: yes\r\n\r\n\0raw"
    );
}

#[test]
fn replacement_accepts_arbitrary_binary_bytes_without_decoding() {
    let mut message = Message::response(
        StatusCode::OK,
        &HeaderMap::new(),
        Bytes::from_static(b"old"),
    );
    let replacement = Bytes::from_static(&[0x00, 0x80, 0xff, b'{']);

    message.replace_body(replacement.clone());

    assert_eq!(message.body, replacement);
    assert!(message.body_modified);
    assert_eq!(message.declared_content_length(), Some(4));
}

#[test]
fn raw_head_parser_preserves_binary_values_case_and_interleaved_duplicates() {
    let head = b"POST /raw HTTP/1.1\r\n\
X-Trace: first\x80\r\n\
x-Other: middle\xff\r\n\
x-TRACE: second\r\n\
x-Other: last\r\n\r\n";

    let message =
        Message::from_raw_http1_head(head, Bytes::from_static(b"body")).expect("raw head");

    assert_eq!(message.start_line, "POST /raw HTTP/1.1");
    assert_eq!(
        message
            .headers
            .iter()
            .map(|header| (header.name.as_ref(), header.value.as_ref()))
            .collect::<Vec<_>>(),
        vec![
            (b"X-Trace".as_slice(), b"first\x80".as_slice()),
            (b"x-Other".as_slice(), b"middle\xff".as_slice()),
            (b"x-TRACE".as_slice(), b"second".as_slice()),
            (b"x-Other".as_slice(), b"last".as_slice()),
        ]
    );
    assert_eq!(
        message.reconstruct(),
        Bytes::from_static(
            b"POST /raw HTTP/1.1\r\n\
X-Trace: first\x80\r\n\
x-Other: middle\xff\r\n\
x-TRACE: second\r\n\
x-Other: last\r\n\r\nbody"
        )
    );
}

#[test]
fn raw_head_parser_preserves_each_header_separator_and_optional_whitespace() {
    let head = b"GET /ows HTTP/1.1\r\n\
X-Mixed:\t  value \t\r\n\
X-Compact:value\r\n\
X-Only-Ows:\t \t\r\n\r\n";

    let message = Message::from_raw_http1_head(head, Bytes::new()).expect("raw head");

    assert_eq!(message.headers[0].value, "value");
    assert_eq!(message.headers[0].leading_ows(), b"\t  ");
    assert_eq!(message.headers[0].trailing_ows(), b" \t");
    assert_eq!(message.headers[1].leading_ows(), b"");
    assert_eq!(message.headers[1].trailing_ows(), b"");
    assert_eq!(message.headers[2].value, "");
    assert_eq!(message.headers[2].leading_ows(), b"\t \t");
    assert_eq!(message.headers[2].trailing_ows(), b"");
    assert_eq!(message.reconstruct(), Bytes::from_static(head));
    assert_eq!(
        message.header_map().expect("semantic headers")["x-mixed"],
        "value"
    );
}

#[test]
fn raw_head_parser_preserves_non_standard_reason_phrase() {
    let message = Message::from_raw_http1_head(
        b"HTTP/1.1 299 Vendor Specific Result\r\nX-Test: yes\r\n\r\n",
        Bytes::new(),
    )
    .expect("raw response head");

    assert_eq!(message.start_line, "HTTP/1.1 299 Vendor Specific Result");
    assert_eq!(message.http_status(), Some(299));
    assert!(
        message
            .reconstruct()
            .starts_with(b"HTTP/1.1 299 Vendor Specific Result\r\n")
    );
}
