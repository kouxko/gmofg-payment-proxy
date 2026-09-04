//! 故障动作对内存报文产生何种精确结果的集成测试。
//!
//! 这里验证错误长度、截断、丢弃等动作的字节/状态语义；不建立真实 TLS 连接，也不把
//! Android 端最终出现的异常类型当作本测试已经证明的内容。

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use intercept_proxy_runtime::fault::{FaultAction, ResponseDisposition, apply_response_actions};
use intercept_proxy_runtime::message::Message;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn content_length_offset_changes_only_the_declared_length() {
    let source = Message::response(
        StatusCode::OK,
        &HeaderMap::new(),
        Bytes::from_static(b"body"),
    );

    let result = apply_response_actions(
        source,
        &[FaultAction::ContentLengthOffset(3)],
        &CancellationToken::new(),
    )
    .await
    .expect("content-length fault");
    let ResponseDisposition::Send { message, .. } = result else {
        panic!("expected send disposition");
    };

    assert_eq!(message.body, Bytes::from_static(b"body"));
    assert_eq!(message.declared_content_length(), Some(7));
}

#[tokio::test]
async fn custom_status_preserves_composite_transfer_encoding_without_content_length() {
    let source = Message::from_raw_http1_head(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\nContent-Length: 99\r\n\r\n",
        Bytes::from_static(b"encoded body"),
    )
    .expect("buffered transfer-coded response");

    let result = apply_response_actions(
        source,
        &[FaultAction::CustomStatus(StatusCode::SERVICE_UNAVAILABLE)],
        &CancellationToken::new(),
    )
    .await
    .expect("custom status fault");
    let ResponseDisposition::Send { message, .. } = result else {
        panic!("expected send disposition");
    };

    assert_eq!(message.http_status(), Some(503));
    assert!(message.uses_transfer_encoding());
    assert_eq!(message.declared_content_length(), None);
    assert!(message.headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case(b"transfer-encoding")
            && header.value.eq_ignore_ascii_case(b"gzip, chunked")
    }));
}

#[tokio::test]
async fn truncate_response_preserves_declared_length_and_selects_a_strict_prefix() {
    let mut headers = HeaderMap::new();
    headers.insert("content-length", "4".parse().expect("valid header"));
    let source = Message::response(StatusCode::OK, &headers, Bytes::from_static(b"body"));

    let result = apply_response_actions(
        source,
        &[FaultAction::TruncateResponse(2)],
        &CancellationToken::new(),
    )
    .await
    .expect("truncate fault");
    let ResponseDisposition::Truncate { message, bytes, .. } = result else {
        panic!("expected truncate disposition");
    };

    assert_eq!(bytes, 2);
    assert_eq!(&message.body[..bytes], b"bo");
    assert_eq!(message.declared_content_length(), Some(4));
}

#[tokio::test]
async fn drop_response_returns_no_message_for_later_processing() {
    let source = Message::response(
        StatusCode::OK,
        &HeaderMap::new(),
        Bytes::from_static(b"body"),
    );

    let result = apply_response_actions(
        source,
        &[FaultAction::DropResponse {
            read_upstream: true,
        }],
        &CancellationToken::new(),
    )
    .await
    .expect("drop fault");

    assert!(matches!(result, ResponseDisposition::Drop));
}
