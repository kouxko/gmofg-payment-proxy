use intercept_proxy_application::{CaptureSort, ChannelId, MessageStage, SortDirection};

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
fn empty_query_arguments_apply_stable_defaults() {
    let http = HttpCaptureArguments::default().into_query();

    assert_eq!(http.sort, CaptureSort::OccurredAt);
    assert_eq!(http.direction, SortDirection::Desc);
    assert_eq!((http.page.page, http.page.page_size), (1, 100));
}
