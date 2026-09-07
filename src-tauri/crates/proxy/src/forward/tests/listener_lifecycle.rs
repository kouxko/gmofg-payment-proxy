#[tokio::test]
async fn bound_listener_stops_idle_connections_on_cancellation() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service =
        ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication)).unwrap();
    let cancellation = CancellationToken::new();
    let run_cancel = cancellation.clone();
    let task = tokio::spawn(async move { service.serve_listener(listener, run_cancel).await });
    let mut client = TcpStream::connect(address).await.unwrap();
    cancellation.cancel();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("listener stop must be bounded")
        .unwrap()
        .unwrap();
    let mut byte = [0u8; 1];
    match client.read(&mut byte).await {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
            ) => {}
        outcome => panic!("cancelled listener must close the client, got {outcome:?}"),
    }
}
