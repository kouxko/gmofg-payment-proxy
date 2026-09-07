use super::*;

#[tokio::test]
async fn incorrect_content_length_tail_write_is_bounded() {
    let (mut client, server) = tokio::io::duplex(256);
    write_test_request(&mut client).await;
    let service = downstream_test_service(
        Bytes::from(vec![b'x'; 4 * 1024]),
        Some(1),
        Duration::from_millis(10),
    );

    let error = tokio::time::timeout(
        Duration::from_secs(1),
        service.run_connection_inner(
            Box::new(server),
            &downstream_test_context(),
            CancellationToken::new(),
        ),
    )
    .await
    .expect("incorrect content-length tail write must be bounded")
    .expect_err("intentional incorrect content-length remains a terminal fault");

    assert_eq!(error.code, ErrorCode::IncorrectContentLength.as_str());
}

#[tokio::test]
async fn incorrect_content_length_tail_write_stops_when_supervisor_cancels() {
    let mut io: BoxIo = Box::new(PendingWriteIo(PendingWriteStage::Tail));
    let cancellation = CancellationToken::new();
    let stop = cancellation.clone();
    let intentional_fault = StdMutex::new(Some(IntentionalWireFault::IncorrectContentLength));

    let ((), result) = tokio::join!(
        async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            stop.cancel();
        },
        finish_downstream_write(
            &mut io,
            Some(Bytes::from_static(b"tail")),
            Duration::from_secs(30),
            &cancellation,
            &intentional_fault,
        )
    );
    let error = result.expect_err("supervisor cancellation must stop the raw tail write");

    assert_eq!(error.code, ErrorCode::ProxyStopped.as_str());
}

#[tokio::test]
async fn downstream_flush_and_shutdown_each_respect_write_timeout() {
    for stage in [PendingWriteStage::Flush, PendingWriteStage::Shutdown] {
        let mut io: BoxIo = Box::new(PendingWriteIo(stage));
        let error = finish_downstream_write(
            &mut io,
            None,
            Duration::from_millis(5),
            &CancellationToken::new(),
            &StdMutex::new(None),
        )
        .await
        .expect_err("a stalled downstream write stage must time out");

        assert_eq!(error.code, ErrorCode::Io.as_str());
        assert!(error.message.contains("timed out after 5 ms"));
    }
}
