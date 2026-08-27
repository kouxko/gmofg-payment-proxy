#[tokio::test]
async fn records_request_response_terminal_events_and_real_metrics() {
    let pipeline = adapter(Vec::new(), 10);
    let epoch = Uuid::new_v4();
    let context = test_context(epoch, Uuid::new_v4(), transaction_channel());

    open_test_connection(&pipeline, &context).await;
    let opened = pipeline.snapshot(Some(epoch)).await.expect("metrics");
    assert_eq!(opened.channels[&transaction_channel()].connected_clients, 1);

    let mut request = request_message(r#"{"amount":100}"#);
    assert!(
        pipeline
            .apply_request_policy(&context, &mut request)
            .await
            .expect("request")
            .is_empty()
    );
    let running = pipeline.snapshot(Some(epoch)).await.expect("metrics");
    assert_eq!(running.channels[&transaction_channel()].request_count, 1);
    assert_eq!(running.active_sessions, 1);

    let mut response = response_message();
    assert!(
        pipeline
            .apply_response_policy(&context, &mut response)
            .await
            .expect("response")
            .is_empty()
    );
    let session_id = pipeline
        .state
        .lock()
        .connection(&context)
        .and_then(|connection| connection.session_id)
        .expect("active session");
    let recorded = pipeline
        .sessions
        .get_record(session_id)
        .expect("recorded session");
    let recorded_request = recorded.detail.request.as_ref().expect("request");
    assert_eq!(recorded_request.http_status, None);
    assert_eq!(recorded_request.start_line_bytes, b"POST /payment HTTP/1.1");
    assert_eq!(recorded_request.headers["x-request-id"], ["REQ-1"]);
    assert_eq!(
        recorded_request.raw_headers,
        vec![
            RawHttpHeaderViewModel {
                name_bytes: b"host".to_vec(),
                value_bytes: b"example.test".to_vec(),
                leading_ows_bytes: b" ".to_vec(),
                trailing_ows_bytes: Vec::new(),
            },
            RawHttpHeaderViewModel {
                name_bytes: b"x-request-id".to_vec(),
                value_bytes: b"REQ-1".to_vec(),
                leading_ows_bytes: b" ".to_vec(),
                trailing_ows_bytes: Vec::new(),
            },
        ]
    );
    let recorded_response = recorded.detail.response.as_ref().expect("response");
    assert_eq!(recorded_response.http_status, Some(200));
    assert_eq!(recorded.detail.summary.http_status, Some(200));
    assert_eq!(recorded_response.start_line_bytes, b"HTTP/1.1 200 OK");
    assert_eq!(recorded_response.headers["x-server"], ["gmo-fg"]);
    assert_eq!(
        recorded_response.raw_headers,
        vec![RawHttpHeaderViewModel {
            name_bytes: b"x-server".to_vec(),
            value_bytes: b"gmo-fg".to_vec(),
            leading_ows_bytes: b" ".to_vec(),
            trailing_ows_bytes: Vec::new(),
        }]
    );
    pipeline.connection_closed(&context, &Ok(())).await;

    let closed = pipeline.snapshot(Some(epoch)).await.expect("metrics");
    assert_eq!(closed.channels[&transaction_channel()].connected_clients, 0);
    assert_eq!(closed.active_sessions, 0);
    assert!(closed.logical_memory_bytes > 0);
    assert!(pipeline.events.current_cursor() > 0);
    assert_eq!(pipeline.sessions.len(), 1);
    let session_id = pipeline
        .state
        .lock()
        .connection(&context)
        .and_then(|connection| connection.session_id);
    assert!(session_id.is_none(), "closed connection state is removed");

    let next_context = test_context(Uuid::new_v4(), Uuid::new_v4(), transaction_channel());
    open_test_connection(&pipeline, &next_context).await;
    let next_epoch = pipeline
        .snapshot(Some(next_context.runtime_epoch))
        .await
        .expect("next epoch metrics");
    assert_eq!(
        next_epoch.channels[&transaction_channel()].request_count,
        0,
        "runtime counters reset for a new epoch"
    );
    pipeline.connection_closed(&next_context, &Ok(())).await;
}

#[tokio::test]
async fn stores_upstream_security_evidence_on_the_active_session() {
    let pipeline = adapter(Vec::new(), 10);
    let context = test_context(Uuid::new_v4(), Uuid::new_v4(), transaction_channel());
    open_test_connection(&pipeline, &context).await;
    pipeline
        .apply_request_policy(&context, &mut request_message(r#"{"amount":100}"#))
        .await
        .unwrap();
    let session_id = pipeline
        .state
        .lock()
        .connection(&context)
        .and_then(|connection| connection.session_id)
        .unwrap();

    pipeline
        .upstream_security_established(&context, &upstream_tls_evidence("CN=upstream.test"))
        .await;
    pipeline.connection_closed(&context, &Ok(())).await;

    let completed = pipeline.sessions.get_record(session_id).unwrap();
    let text = &completed.detail.proxy_to_server_tls;
    assert!(text.contains("TLS 1.2"));
    assert!(text.contains("CN=upstream.test"));
    assert!(text.contains("AA:BB:CC"));
    assert!(text.contains("已配置、已提交"));
}

#[tokio::test]
async fn reports_upstream_security_evidence_when_connection_has_no_session() {
    let pipeline = adapter(Vec::new(), 10);
    let context = test_context(Uuid::new_v4(), Uuid::new_v4(), transaction_channel());
    open_test_connection(&pipeline, &context).await;

    pipeline
        .upstream_security_established(&context, &upstream_tls_evidence("CN=orphan-upstream.test"))
        .await;

    let replay = pipeline.events.replay_after(0);
    let failure = replay.events.iter().find_map(|event| match &event.payload {
        UiEventPayload::OperationFailed(error)
            if error.code == "UPSTREAM_SECURITY_SESSION_MISSING" =>
        {
            Some(error)
        }
        _ => None,
    });
    let failure = failure.expect("missing session must be reported to the UI event stream");
    assert_eq!(
        failure.entity_id.as_deref(),
        Some(&*context.connection_id.to_string())
    );
    assert_eq!(failure.runtime_epoch, Some(context.runtime_epoch));
    assert!(failure.message.contains("CN=orphan-upstream.test"));
    assert!(failure.message.contains("TLS 1.2"));
}

#[tokio::test]
async fn reports_pre_session_tls_failure_with_listener_context() {
    let pipeline = adapter(Vec::new(), 10);
    let context = test_context(Uuid::new_v4(), Uuid::new_v4(), dll_channel());

    pipeline
        .connection_closed(
            &context,
            &Err(ProxyError::new(
                ErrorCode::DownstreamTlsHandshakeFailed,
                "peer is incompatible: no cipher suites in common",
            )),
        )
        .await;

    let replay = pipeline.events.replay_after(0);
    let failure = replay.events.iter().find_map(|event| match &event.payload {
        UiEventPayload::OperationFailed(error)
            if error.code == "DOWNSTREAM_TLS_HANDSHAKE_FAILED" =>
        {
            Some(error)
        }
        _ => None,
    });
    let failure = failure.expect("pre-session TLS failure must reach diagnostics");
    assert_eq!(failure.entity_id.as_deref(), Some(dll_channel().as_str()));
    assert_eq!(failure.runtime_epoch, Some(context.runtime_epoch));
    assert!(failure.message.contains("no cipher suites in common"));
}

#[tokio::test]
async fn reports_capacity_failure_and_keeps_previous_upstream_security_evidence() {
    let pipeline = adapter(Vec::new(), 10);
    let context = test_context(Uuid::new_v4(), Uuid::new_v4(), transaction_channel());
    open_test_connection(&pipeline, &context).await;
    pipeline
        .apply_request_policy(&context, &mut request_message(r#"{"amount":100}"#))
        .await
        .unwrap();
    let session_id = pipeline
        .state
        .lock()
        .connection(&context)
        .and_then(|connection| connection.session_id)
        .unwrap();
    let before = pipeline.sessions.get_record(session_id).unwrap();
    pipeline
        .sessions
        .set_limits(10, pipeline.sessions.logical_bytes())
        .expect("current session must fit the exact capacity limit");

    let subject = format!("CN={}.test", "capacity".repeat(128));
    pipeline
        .upstream_security_established(&context, &upstream_tls_evidence(&subject))
        .await;

    let after = pipeline.sessions.get_record(session_id).unwrap();
    assert_eq!(
        after.detail.proxy_to_server_tls, before.detail.proxy_to_server_tls,
        "failed upsert must roll the session record back"
    );

    let replay = pipeline.events.replay_after(0);
    assert!(replay.events.iter().any(|event| {
        matches!(
            &event.payload,
            UiEventPayload::ResourceWarning { message }
                if message.contains("无法保存已建立的上游安全证据")
                    && message.contains(&subject)
        )
    }));
    let failure = replay.events.iter().find_map(|event| match &event.payload {
        UiEventPayload::OperationFailed(error) if error.code == "RESOURCE_EXHAUSTED" => Some(error),
        _ => None,
    });
    let failure = failure.expect("capacity failure must be reported to the UI event stream");
    assert_eq!(failure.entity_id.as_deref(), Some(&*session_id.to_string()));
    assert_eq!(failure.runtime_epoch, Some(context.runtime_epoch));
    assert!(failure.message.contains(&subject));
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn isolates_interleaved_workspace_metrics_and_ignores_late_stop_events() {
    let pipeline = adapter(Vec::new(), 10);
    let epoch_a = Uuid::new_v4();
    let epoch_b = Uuid::new_v4();
    let context_a = test_context(epoch_a, Uuid::new_v4(), transaction_channel());
    let context_b = test_context(epoch_b, Uuid::new_v4(), dll_channel());

    open_test_connection(&pipeline, &context_a).await;
    open_test_connection(&pipeline, &context_b).await;

    let mut request_a = request_message(r#"{"workspace":"a"}"#);
    pipeline
        .apply_request_policy(&context_a, &mut request_a)
        .await
        .expect("workspace A request");
    let mut request_b = request_message(r#"{"workspace":"b"}"#);
    pipeline
        .apply_request_policy(&context_b, &mut request_b)
        .await
        .expect("workspace B request");
    let mut response_b = response_message();
    pipeline
        .apply_response_policy(&context_b, &mut response_b)
        .await
        .expect("workspace B response");
    pipeline
        .runtime_fault(
            epoch_a,
            transaction_channel(),
            &ProxyError::new(ErrorCode::Io, "workspace A fault"),
        )
        .await;

    let metrics_a = pipeline.snapshot(Some(epoch_a)).await.expect("A metrics");
    assert_eq!(metrics_a.channels.len(), 1);
    assert_eq!(
        metrics_a.channels[&transaction_channel()],
        ChannelRuntimeMetrics {
            connected_clients: 1,
            request_count: 1,
            error_count: 1,
            upstream_response_count: 0,
            last_upstream_error: None,
        }
    );
    let metrics_b = pipeline.snapshot(Some(epoch_b)).await.expect("B metrics");
    assert_eq!(metrics_b.channels.len(), 1);
    assert_eq!(
        metrics_b.channels[&dll_channel()],
        ChannelRuntimeMetrics {
            connected_clients: 1,
            request_count: 1,
            error_count: 0,
            upstream_response_count: 1,
            last_upstream_error: None,
        }
    );

    pipeline.runtime_stopping(epoch_a).await;
    assert!(
        pipeline
            .snapshot(Some(epoch_a))
            .await
            .expect("stopped A metrics")
            .channels
            .is_empty()
    );
    assert_eq!(
        pipeline
            .snapshot(Some(epoch_b))
            .await
            .expect("B survives A stop")
            .channels[&dll_channel()]
            .connected_clients,
        1
    );

    pipeline
        .connection_closed(
            &context_a,
            &Err(ProxyError::new(
                ErrorCode::UpstreamReadTimeout,
                "late workspace A close",
            )),
        )
        .await;
    pipeline
        .runtime_fault(
            epoch_a,
            transaction_channel(),
            &ProxyError::new(ErrorCode::Io, "late workspace A fault"),
        )
        .await;
    let late_context_a = test_context(epoch_a, Uuid::new_v4(), transaction_channel());
    pipeline.connection_opened(&late_context_a).await;
    let mut late_request_a = request_message(r#"{"workspace":"late-a"}"#);
    let late_request_error = pipeline
        .apply_request_policy(&late_context_a, &mut late_request_a)
        .await
        .expect_err("stopped epoch must reject a late request");
    assert_eq!(late_request_error.code, ErrorCode::ProxyStopped.as_str());
    assert!(
        pipeline
            .snapshot(Some(epoch_a))
            .await
            .expect("late A events")
            .channels
            .is_empty(),
        "late close, fault, or open must not recreate a stopped epoch"
    );

    let aggregate = pipeline.snapshot(None).await.expect("aggregate metrics");
    assert_eq!(aggregate.channels.len(), 1);
    assert_eq!(
        aggregate.channels[&dll_channel()],
        metrics_b.channels[&dll_channel()]
    );

    pipeline.connection_closed(&context_b, &Ok(())).await;
    let closed_b = pipeline
        .snapshot(Some(epoch_b))
        .await
        .expect("closed B metrics");
    assert_eq!(closed_b.channels[&dll_channel()].connected_clients, 0);
    assert_eq!(closed_b.channels[&dll_channel()].request_count, 1);
    assert_eq!(closed_b.channels[&dll_channel()].upstream_response_count, 1);
}
