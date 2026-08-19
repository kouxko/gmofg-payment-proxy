use chrono::{TimeZone as _, Utc};
use intercept_proxy_application::{
    CaptureSort, ChannelId, ListenerId, MessageStage, ProtocolDirection, ProtocolPackageId,
    ProtocolPackageRef, ProtocolPackageVersion, SocketCaptureKind, SocketCaptureSort,
    SocketConnectionId, SortDirection, WorkspaceId,
};

use super::*;

#[test]
fn diagnostic_arguments_preserve_filters_and_default_limit() {
    let query = DiagnosticArguments {
        keyword: Some("timeout".into()),
        after_event_id: Some(41),
        limit: None,
    }
    .into_query();

    assert_eq!(query.keyword.as_deref(), Some("timeout"));
    assert_eq!(query.after_event_id, Some(41));
    assert_eq!(query.limit, 300);
}

#[test]
fn http_capture_arguments_preserve_filters_and_explicit_paging() {
    let channel = ChannelId::new("payment").unwrap();
    let rule_id = "00000000-0000-0000-0000-000000000011".parse().unwrap();
    let query = HttpCaptureArguments {
        keyword: Some("approved".into()),
        terminal_ip: Some("127.0.0.1".into()),
        channel: Some(channel.clone()),
        stage: Some(MessageStage::Response),
        result: Some("success".into()),
        rule_id: Some(rule_id),
        after_event_id: Some(7),
        sort: Some(CaptureSort::Duration),
        direction: Some(SortDirection::Asc),
        page: Some(3),
        page_size: Some(25),
    }
    .into_query();

    assert_eq!(query.keyword.as_deref(), Some("approved"));
    assert_eq!(query.terminal_ip.as_deref(), Some("127.0.0.1"));
    assert_eq!(query.channel, Some(channel));
    assert_eq!(query.stage, Some(MessageStage::Response));
    assert_eq!(query.result.as_deref(), Some("success"));
    assert_eq!(query.rule_id, Some(rule_id));
    assert_eq!(query.after_event_id, Some(7));
    assert_eq!(query.sort, CaptureSort::Duration);
    assert_eq!(query.direction, SortDirection::Asc);
    assert_eq!(query.page.page, 3);
    assert_eq!(query.page.page_size, 25);
}

#[test]
fn socket_capture_arguments_preserve_directional_identity_and_paging() {
    let package = ProtocolPackageRef {
        id: ProtocolPackageId::new("iso8583-standard").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    };
    let occurred_from = Utc.with_ymd_and_hms(2026, 8, 19, 3, 4, 5).unwrap();
    let occurred_to = Utc.with_ymd_and_hms(2026, 8, 19, 4, 5, 6).unwrap();
    let query = SocketCaptureArguments {
        workspace_id: Some(WorkspaceId::new()),
        listener_id: Some(ListenerId::new()),
        session_id: Some("00000000-0000-0000-0000-000000000013".parse().unwrap()),
        connection_id: Some(SocketConnectionId::new()),
        package: Some(package.clone()),
        direction: Some(ProtocolDirection::Downstream),
        kind: Some(SocketCaptureKind::LocalExchange),
        occurred_from: Some(occurred_from),
        occurred_to: Some(occurred_to),
        sort: Some(SocketCaptureSort::Size),
        direction_sort: Some(SortDirection::Asc),
        page: Some(4),
        page_size: Some(50),
    }
    .into_query();

    assert_eq!(query.package, Some(package));
    assert_eq!(query.direction, Some(ProtocolDirection::Downstream));
    assert_eq!(query.kind, Some(SocketCaptureKind::LocalExchange));
    assert_eq!(query.occurred_from, Some(occurred_from));
    assert_eq!(query.occurred_to, Some(occurred_to));
    assert_eq!(query.sort, SocketCaptureSort::Size);
    assert_eq!(query.direction_sort, SortDirection::Asc);
    assert_eq!(query.page.page, 4);
    assert_eq!(query.page.page_size, 50);
}

#[test]
fn empty_query_arguments_apply_stable_defaults() {
    let http = HttpCaptureArguments::default().into_query();
    let socket = SocketCaptureArguments::default().into_query();

    assert_eq!(http.sort, CaptureSort::OccurredAt);
    assert_eq!(http.direction, SortDirection::Desc);
    assert_eq!((http.page.page, http.page.page_size), (1, 100));
    assert_eq!(socket.sort, SocketCaptureSort::OccurredAt);
    assert_eq!(socket.direction_sort, SortDirection::Desc);
    assert_eq!((socket.page.page, socket.page.page_size), (1, 100));
}
