use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

async fn run_wire_request(request: &'static [u8]) -> (Result<()>, Vec<u8>) {
    let (mut client, server) = tokio::io::duplex(4 * 1024);
    let service = downstream_test_service(Bytes::new(), None, Duration::from_secs(1));
    let context = downstream_test_context();

    let client_task = async move {
        client.write_all(request).await.expect("write request");
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .await
            .expect("read response");
        response
    };
    let (result, response) = tokio::join!(
        service.run_connection_inner(Box::new(server), &context, CancellationToken::new(),),
        client_task,
    );
    (result, response)
}

#[tokio::test]
async fn fixed_server_returns_501_for_connect_and_upgrade() {
    for request in [
        &b"CONNECT upstream.test:443 HTTP/1.1\r\nHost: upstream.test:443\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"[..],
        &b"GET /chat HTTP/1.1\r\nHost: upstream.test\r\nConnection: Upgrade, close\r\nUpgrade: websocket\r\nContent-Length: 0\r\n\r\n"[..],
    ] {
        let (result, response) = run_wire_request(request).await;

        result.expect("an intentional 501 response is not a connection failure");
        assert!(
            response.starts_with(b"HTTP/1.1 501"),
            "expected a 501 response, got {response:?}"
        );
        assert!(
            String::from_utf8_lossy(&response)
                .contains("HTTP CONNECT and Upgrade are not supported")
        );
    }
}

#[tokio::test]
async fn handler_failure_marks_the_connection_exchange_task_failed() {
    let (mut client, server) = tokio::io::duplex(4 * 1024);
    let mut service = downstream_test_service(Bytes::new(), None, Duration::from_secs(1));
    service.limits.max_body_bytes = 1;
    let context = downstream_test_context();
    let task_scope = crate::listener::ConnectionTaskScope::new();

    let client_task = async move {
        client
            .write_all(
                b"POST / HTTP/1.1\r\nHost: proxy.test\r\nConnection: close\r\nContent-Length: 2\r\n\r\nxx",
            )
            .await
            .expect("write invalid request");
        let mut ignored = Vec::new();
        client.read_to_end(&mut ignored).await.expect("read EOF");
    };
    let (result, ()) = tokio::join!(
        service.run_connection_inner_in_scope(
            Box::new(server),
            &context,
            CancellationToken::new(),
            &task_scope,
        ),
        client_task,
    );
    let error = result.expect_err("header validation must fail");
    task_scope.close();
    task_scope.drain().await;
    let aggregate = task_scope.snapshot().aggregate;

    assert_eq!(error.code, ErrorCode::BodyTooLarge.as_str());
    let (_, exchange_error) = aggregate
        .lowest_error
        .expect("the connection Exchange task must close with the handler failure");
    assert_eq!(exchange_error.code, ErrorCode::BodyTooLarge.as_str());
}
