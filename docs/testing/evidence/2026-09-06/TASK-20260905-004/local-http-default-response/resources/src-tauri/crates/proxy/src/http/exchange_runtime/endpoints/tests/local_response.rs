use super::*;
use crate::RawHeader;

#[tokio::test]
async fn local_http_default_response_preserves_effective_bytes_and_only_content_headers() {
    for body in [b"".as_slice(), b"\x00\xff\x81", "売上".as_bytes()] {
        let content_headers = vec![
            RawHeader::with_wire_ows(
                b"cOnTent-Type".to_vec(),
                b"application/octet-stream".to_vec(),
                b"\t".to_vec(),
                b" ".to_vec(),
            )
            .unwrap(),
            RawHeader::new(b"Content-Encoding".to_vec(), b"identity".to_vec()),
        ];
        let mut headers = content_headers.clone();
        for (name, value) in [
            ("Host", "localhost:9"),
            ("Authorization", "test-only"),
            ("Cookie", "test-only"),
            ("X-Request", "do-not-echo"),
            ("Content-Length", "999"),
            ("Transfer-Encoding", "chunked"),
        ] {
            headers.push(RawHeader::new(
                name.as_bytes().to_vec(),
                value.as_bytes().to_vec(),
            ));
        }
        let message = Message {
            start_line: "POST /payment HTTP/1.1".into(),
            headers,
            body: Bytes::copy_from_slice(body),
            body_modified: true,
        };
        let (receipt, committed) = crate::http::upstream_write_receipt();
        let exchange = LocalHttpServerConnector
            .send(
                &connection_context(),
                &NoopPipelinePorts,
                ForwardRequest {
                    method: Method::POST,
                    uri: "/payment".parse().unwrap(),
                    message,
                },
                &[],
                receipt,
                None,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        let response = exchange.final_response;
        println!(
            "Local input={body:?}, response head={:?}, body={:?}",
            response.headers, response.body
        );
        assert_eq!(response.start_line, "HTTP/1.1 200 OK");
        assert_eq!(response.body.as_ref(), body);
        assert_eq!(response.headers.len(), 3);
        assert_eq!(&response.headers[..2], &content_headers);
        assert_eq!(response.declared_content_length(), Some(body.len()));
        assert!(!response.uses_transfer_encoding());
        assert!(exchange.informational_heads.is_empty());
        assert_eq!(
            committed.wait(&CancellationToken::new()).await.unwrap(),
            WriteOutcome::Committed(())
        );
    }
}
