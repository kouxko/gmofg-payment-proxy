use super::*;

#[tokio::test]
async fn transparent_relay_preserves_arbitrary_write_boundaries_and_coalesced_frames() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let request_frames = [
        vec![0x00, 0xff, 0x10],
        (0..16_411_u32)
            .map(|index| index.wrapping_mul(197).to_le_bytes()[0])
            .collect::<Vec<_>>(),
        b"third-frame\0with-binary\xff".to_vec(),
    ];
    let upstream_expected_request = request_frames.concat();
    let response_frames = [
        b"response-one".to_vec(),
        vec![0x00, 0xff, 0x80, 0x7f],
        (0..8_213_u32)
            .map(|index| index.wrapping_mul(89).to_be_bytes()[3])
            .collect::<Vec<_>>(),
    ];
    let expected_response = response_frames.concat();
    let upstream_response_frames = response_frames.clone();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut received = Vec::new();
        stream.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, upstream_expected_request);

        for frame in upstream_response_frames {
            for chunk in frame.chunks(3) {
                stream.write_all(chunk).await.unwrap();
                tokio::task::yield_now().await;
            }
        }
        stream.shutdown().await.unwrap();
    });

    let bind_addr = reserve_address();
    let service = Arc::new(
        SocketRelayService::build(transparent_config(bind_addr, upstream_address)).unwrap(),
    );
    let cancellation = CancellationToken::new();
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { service.serve(server_cancel).await });

    let mut client = connect_retry(bind_addr).await;
    for (frame_index, frame) in request_frames.iter().enumerate() {
        let chunk_size = [1, 257, 5][frame_index];
        for chunk in frame.chunks(chunk_size) {
            client.write_all(chunk).await.unwrap();
            tokio::task::yield_now().await;
        }
    }
    client.shutdown().await.unwrap();
    let mut actual_response = Vec::new();
    client.read_to_end(&mut actual_response).await.unwrap();

    assert_eq!(actual_response, expected_response);
    cancellation.cancel();
    server.await.unwrap().unwrap();
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn transparent_relay_supports_simultaneous_bidirectional_binary_traffic() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let client_payload = Arc::new(
        (0..65_537_u32)
            .map(|index| index.wrapping_mul(131).to_le_bytes()[0])
            .collect::<Vec<_>>(),
    );
    let server_payload = Arc::new(
        (0..65_539_u32)
            .map(|index| index.wrapping_mul(193).to_be_bytes()[3])
            .collect::<Vec<_>>(),
    );
    let expected_client_payload = Arc::clone(&client_payload);
    let outgoing_server_payload = Arc::clone(&server_payload);
    let upstream_task = tokio::spawn(async move {
        let (stream, _) = upstream.accept().await.unwrap();
        let (mut reader, mut writer) = stream.into_split();
        let receive = async {
            let mut received = Vec::new();
            reader.read_to_end(&mut received).await.unwrap();
            assert_eq!(received.as_slice(), expected_client_payload.as_slice());
        };
        let send = async {
            writer
                .write_all(outgoing_server_payload.as_slice())
                .await
                .unwrap();
            writer.shutdown().await.unwrap();
        };
        tokio::join!(receive, send);
    });

    let bind_addr = reserve_address();
    let service = Arc::new(
        SocketRelayService::build(transparent_config(bind_addr, upstream_address)).unwrap(),
    );
    let cancellation = CancellationToken::new();
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { service.serve(server_cancel).await });

    let client = connect_retry(bind_addr).await;
    let (mut reader, mut writer) = client.into_split();
    let outgoing_client_payload = Arc::clone(&client_payload);
    let expected_server_payload = Arc::clone(&server_payload);
    tokio::time::timeout(Duration::from_secs(3), async {
        let send = async {
            writer
                .write_all(outgoing_client_payload.as_slice())
                .await
                .unwrap();
            writer.shutdown().await.unwrap();
        };
        let receive = async {
            let mut received = Vec::new();
            reader.read_to_end(&mut received).await.unwrap();
            assert_eq!(received.as_slice(), expected_server_payload.as_slice());
        };
        tokio::join!(send, receive);
    })
    .await
    .expect("simultaneous direct relay traffic must not deadlock");

    cancellation.cancel();
    server.await.unwrap().unwrap();
    upstream_task.await.unwrap();
}
