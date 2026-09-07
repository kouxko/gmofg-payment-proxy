use super::*;
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn consumed_response_body_does_not_claim_transport_flush() {
    let tracker = Arc::new(ResponseWriteTracker::default());
    let body = WireBody::new(
        Bytes::from_static(b"response"),
        8,
        TrafficSchedule::default(),
        CancellationToken::new(),
    );
    let mut body = ResponseBody::new(body, Arc::clone(&tracker), None, false, false).unwrap();
    while let Some(frame) = body.frame().await {
        frame.unwrap();
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(5), tracker.wait_for_clear())
            .await
            .is_err(),
        "Body EOF only prepares bytes; the transport has not written or flushed them",
    );
}

#[tokio::test]
async fn response_receipt_completes_only_after_transport_flush() {
    let tracker = Arc::new(ResponseWriteTracker::default());
    let (completed, mut receipt) = tokio::sync::oneshot::channel();
    let body = WireBody::new(
        Bytes::from_static(b"response"),
        8,
        TrafficSchedule::default(),
        CancellationToken::new(),
    );
    let mut body =
        ResponseBody::new(body, Arc::clone(&tracker), Some(completed), false, false).unwrap();
    while let Some(frame) = body.frame().await {
        frame.unwrap();
    }
    assert!(matches!(
        receipt.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    let (io, mut peer) = tokio::io::duplex(128);
    let mut io = ResponseIo::new(Box::new(io), tracker);
    io.write_all(b"response").await.unwrap();
    assert!(matches!(
        receipt.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    io.flush().await.unwrap();
    receipt.await.unwrap().unwrap();
    let mut actual = [0; 8];
    peer.read_exact(&mut actual).await.unwrap();
    println!("downstream flushed peer bytes: {actual:?}");
    assert_eq!(&actual, b"response");
}

#[tokio::test]
async fn incomplete_response_write_propagates_failure_without_success_receipt() {
    let tracker = Arc::new(ResponseWriteTracker::default());
    let (completed, mut receipt) = tokio::sync::oneshot::channel();
    let body = WireBody::new(
        Bytes::from_static(b"response"),
        8,
        TrafficSchedule::default(),
        CancellationToken::new(),
    );
    let mut body =
        ResponseBody::new(body, Arc::clone(&tracker), Some(completed), false, false).unwrap();
    while let Some(frame) = body.frame().await {
        frame.unwrap();
    }
    let mut io = ResponseIo::new(
        Box::new(PendingWriteIo(PendingWriteStage::Flush)),
        Arc::clone(&tracker),
    );
    io.write_all(b"response").await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(5), io.flush())
            .await
            .is_err()
    );
    assert!(matches!(
        receipt.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    let error = ProxyError::new(ErrorCode::Io, "flush deadline exceeded");
    tracker.fail(&error);
    let actual = receipt.await.unwrap().unwrap_err();
    println!(
        "downstream flush failure: code={}, message={}",
        actual.code, actual.message
    );
    assert_eq!(actual.code, error.code);
    assert_eq!(actual.message, error.message);
}
