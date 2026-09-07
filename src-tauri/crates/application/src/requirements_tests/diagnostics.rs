use super::*;

fn diagnostic_entry(summary: &str) -> DiagnosticLogEntryViewModel {
    DiagnosticLogEntryViewModel {
        level: DiagnosticLogLevel::Info,
        stage: DiagnosticLogStage::System,
        summary: summary.into(),
        detail: None,
        device_serial: None,
        listener_id: None,
        profile_id: None,
        socket_context: None,
    }
}

#[test]
fn diagnostic_logs_are_sorted_by_time_then_event_id_newest_first() {
    let events = Arc::new(EventHub::default());
    let application =
        application_with_fake_ports_and_events(Arc::new(FakePorts::default()), Arc::clone(&events));
    let same_time = Utc.with_ymd_and_hms(2026, 8, 27, 4, 0, 0).unwrap();
    events.publish(
        None,
        same_time + chrono::Duration::seconds(1),
        None,
        None,
        UiEventPayload::DiagnosticLogAdded(Box::new(diagnostic_entry("latest-time"))),
    );
    events.publish(
        None,
        same_time,
        None,
        None,
        UiEventPayload::DiagnosticLogAdded(Box::new(diagnostic_entry("same-time-older-id"))),
    );
    events.publish(
        None,
        same_time,
        None,
        None,
        UiEventPayload::DiagnosticLogAdded(Box::new(diagnostic_entry("same-time-newer-id"))),
    );

    let page = application.diagnostic_log_query(&DiagnosticLogQuery::default());
    assert_eq!(
        page.rows
            .iter()
            .map(|row| row.summary.as_str())
            .collect::<Vec<_>>(),
        vec!["latest-time", "same-time-newer-id", "same-time-older-id"]
    );
}

#[test]
fn diagnostic_log_query_returns_rust_labels_and_filters_in_rust() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    application.diagnostic_log_record(DiagnosticLogEntryViewModel {
        level: DiagnosticLogLevel::Info,
        stage: DiagnosticLogStage::AdbForwardControl,
        summary: "ADB forward 控制通道已建立".into(),
        detail: Some("本地端口已转发到设备控制 socket".into()),
        device_serial: Some("device-001".into()),
        listener_id: None,
        profile_id: Some("profile-001".into()),
        socket_context: None,
    });
    application.diagnostic_log_record(DiagnosticLogEntryViewModel {
        level: DiagnosticLogLevel::Error,
        stage: DiagnosticLogStage::DesktopDns,
        summary: "桌面 DNS 解析失败".into(),
        detail: Some("未解析到地址".into()),
        device_serial: None,
        listener_id: Some("listener-001".into()),
        profile_id: None,
        socket_context: None,
    });

    let page = application.diagnostic_log_query(&DiagnosticLogQuery {
        keyword: Some("device-001".into()),
        after_event_id: None,
        limit: 10,
    });
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].stage_text, "ADB 控制通道");
    assert_eq!(page.rows[0].level_text, "信息");
    assert_eq!(page.rows[0].ui_tone, UiTone::Info);
    assert_eq!(page.oldest_retained_event_id, Some(1));
    assert!(!page.snapshot_required);
}

#[test]
fn diagnostic_log_dto_does_not_define_sensitive_payload_fields() {
    let entry = DiagnosticLogEntryViewModel {
        level: DiagnosticLogLevel::Warning,
        stage: DiagnosticLogStage::Cleanup,
        summary: "清理结果".into(),
        detail: Some("已释放端口映射".into()),
        device_serial: None,
        listener_id: None,
        profile_id: None,
        socket_context: None,
    };
    let serialized = serde_json::to_string(&entry).expect("diagnostic entry serializes");
    for forbidden in ["payload", "password", "private_key", "pkcs12"] {
        assert!(!serialized.contains(forbidden));
    }
}

#[test]
fn diagnostic_log_boundary_redacts_secrets_and_limits_text() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let long_base64 = "A".repeat(180);
    application.diagnostic_log_record(DiagnosticLogEntryViewModel {
        level: DiagnosticLogLevel::Error,
        stage: DiagnosticLogStage::UpstreamTls,
        summary: format!("password=plain-secret {}", "过长摘要".repeat(80)),
        detail: Some(format!(
            "pkcs12_password: p12-secret\n-----BEGIN PRIVATE KEY-----\nprivate-material\n-----END PRIVATE KEY-----\n{long_base64}"
        )),
        device_serial: Some("device-001".into()),
        listener_id: None,
        profile_id: None,
        socket_context: None,
    });

    let page = application.diagnostic_log_query(&DiagnosticLogQuery::default());
    let row = page.rows.first().expect("diagnostic row");
    let serialized = serde_json::to_string(row).expect("diagnostic row serializes");
    for secret in [
        "plain-secret",
        "p12-secret",
        "private-material",
        &long_base64,
    ] {
        assert!(!serialized.contains(secret), "secret leaked: {secret}");
    }
    assert!(serialized.contains("已脱敏"));
    assert!(row.summary.chars().count() <= DIAGNOSTIC_SUMMARY_MAX_CHARS);
    assert!(
        row.detail.as_deref().unwrap_or_default().chars().count() <= DIAGNOSTIC_DETAIL_MAX_CHARS
    );
}
