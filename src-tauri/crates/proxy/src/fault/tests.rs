use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use tokio_util::sync::CancellationToken;

use super::*;
use crate::ErrorCode;

#[tokio::test]
async fn arbitrary_replacement_body_rebuilds_length_without_decoding() {
    let source = Message::response(StatusCode::OK, &HeaderMap::new(), Bytes::new());
    let replacement = Bytes::from_static(&[0x00, 0x80, 0xff]);
    let result = apply_response_actions(
        source,
        &[FaultAction::ReplaceBody {
            body: replacement.clone(),
        }],
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let ResponseDisposition::Send { message, .. } = result else {
        panic!("expected response");
    };
    assert_eq!(message.declared_content_length(), Some(3));
    assert_eq!(message.body, replacement);
}

#[test]
fn observation_projection_matches_effective_status_body_length_and_truncation() {
    let source = Message::response(
        StatusCode::OK,
        &HeaderMap::new(),
        Bytes::from_static(b"abcdef"),
    );
    let observed = project_response_for_observation(
        source,
        &[
            FaultAction::CustomStatus(StatusCode::SERVICE_UNAVAILABLE),
            FaultAction::ReplaceBody {
                body: Bytes::from_static(b"12345"),
            },
            FaultAction::ContentLengthOffset(2),
            FaultAction::TruncateResponse(3),
        ],
    )
    .expect("投影成功")
    .expect("响应没有丢弃");
    assert_eq!(
        observed.http_status(),
        Some(StatusCode::SERVICE_UNAVAILABLE.as_u16())
    );
    assert_eq!(observed.body, Bytes::from_static(b"123"));
    assert_eq!(observed.declared_content_length(), Some(7));
}

#[test]
fn observation_projection_reports_drop_and_mock() {
    let source = Message::response(StatusCode::OK, &HeaderMap::new(), Bytes::new());
    assert!(
        project_response_for_observation(
            source.clone(),
            &[FaultAction::DropResponse {
                read_upstream: true,
            }],
        )
        .unwrap()
        .is_none()
    );
    let observed = project_response_for_observation(
        source,
        &[FaultAction::MockResponse {
            status: StatusCode::CREATED,
            headers: HeaderMap::new(),
            body: Bytes::from_static(b"mock"),
        }],
    )
    .unwrap()
    .unwrap();
    assert_eq!(observed.http_status(), Some(StatusCode::CREATED.as_u16()));
    assert_eq!(observed.body, Bytes::from_static(b"mock"));
}

#[tokio::test]
async fn truncation_requires_strict_prefix() {
    let source = Message::response(
        StatusCode::OK,
        &HeaderMap::new(),
        Bytes::from_static(b"123"),
    );
    assert!(
        apply_response_actions(
            source,
            &[FaultAction::TruncateResponse(3)],
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
}

// ACTION-008~010, ACTION-013, TEST-FAULT:
// response mutations execute in order and the final declared length is observable on wire.
#[tokio::test]
async fn response_status_body_and_declared_length_faults_compose_in_order() {
    let source = Message::response(StatusCode::OK, &HeaderMap::new(), Bytes::from_static(b"{}"));
    let result = apply_response_actions(
        source,
        &[
            FaultAction::ReplaceBody {
                body: Bytes::from_static(b"{"),
            },
            FaultAction::CustomStatus(StatusCode::SERVICE_UNAVAILABLE),
            FaultAction::ContentLengthOffset(5),
        ],
        &CancellationToken::new(),
    )
    .await
    .expect("compose response faults");
    let ResponseDisposition::Send { message, .. } = result else {
        panic!("expected response to be sent");
    };
    assert_eq!(message.start_line, "HTTP/1.1 503 Service Unavailable");
    assert_eq!(message.body, Bytes::from_static(b"{"));
    assert_eq!(message.body.len(), 1);
    assert_eq!(message.declared_content_length(), Some(6));
}

// ACTION-006, ENGINE-006, TEST-FAULT:
// a terminal response disposition prevents every later mutation.
#[tokio::test]
async fn drop_response_short_circuits_later_response_actions() {
    let source = Message::response(
        StatusCode::OK,
        &HeaderMap::new(),
        Bytes::from_static(b"original"),
    );
    let result = apply_response_actions(
        source,
        &[
            FaultAction::DropResponse {
                read_upstream: true,
            },
            FaultAction::ReplaceBody {
                body: Bytes::from_static(b"{later"),
            },
        ],
        &CancellationToken::new(),
    )
    .await
    .expect("drop response");
    assert!(matches!(result, ResponseDisposition::Drop));
}

// ACTION-012, STATE-012, TEST-FAULT:
// delays remain cancellable and never keep shutdown waiting for the full configured duration.
#[tokio::test]
async fn delay_observes_proxy_cancellation() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let source = Message::response(StatusCode::OK, &HeaderMap::new(), Bytes::new());
    let error = apply_response_actions(
        source,
        &[FaultAction::Delay(Duration::from_mins(1))],
        &cancellation,
    )
    .await
    .expect_err("cancelled delay");
    assert_eq!(error.code, ErrorCode::ProxyStopped.as_str());
}

#[tokio::test]
async fn request_stage_faults_keep_stage_specific_errors() {
    let source = Message::response(StatusCode::OK, &HeaderMap::new(), Bytes::new());
    let action = FaultAction::DisconnectBeforeUpstream;

    let observation_error =
        project_response_for_observation(source.clone(), std::slice::from_ref(&action))
            .expect_err("request fault cannot be projected as a response");
    assert_eq!(observation_error.code, ErrorCode::ConfigInvalid.as_str());
    assert_eq!(
        observation_error.message,
        "request-stage fault used during response observation"
    );

    let processing_error = apply_response_actions(
        source,
        std::slice::from_ref(&action),
        &CancellationToken::new(),
    )
    .await
    .expect_err("request fault cannot execute during response processing");
    assert_eq!(processing_error.code, ErrorCode::ConfigInvalid.as_str());
    assert_eq!(
        processing_error.message,
        "request-stage fault used during response processing"
    );
}
