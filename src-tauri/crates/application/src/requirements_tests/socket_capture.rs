use super::*;

fn query(page: u32, page_size: u32) -> SocketCaptureQuery {
    SocketCaptureQuery {
        workspace_id: None,
        listener_id: None,
        session_id: None,
        connection_id: None,
        package: None,
        direction: None,
        kind: None,
        occurred_from: None,
        occurred_to: None,
        sort: SocketCaptureSort::OccurredAt,
        direction_sort: SortDirection::Desc,
        page: PageRequest { page, page_size },
    }
}

#[tokio::test]
async fn t27_socket_query_normalizes_paging_without_using_http_capture_query() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());

    let page = application
        .socket_capture_query(query(0, 5_000))
        .await
        .unwrap();

    assert_eq!((page.page, page.page_size), (1, 200));
    let captured = ports.socket_capture_queries.lock();
    assert_eq!(captured.len(), 1);
    assert_eq!(
        (captured[0].page.page, captured[0].page.page_size),
        (1, 200)
    );
}

#[tokio::test]
async fn t27_socket_clear_requires_confirmation_and_reports_completed_count() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let first = workspaces
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|workspace| workspace.selected)
        .unwrap();
    let second = workspaces.create("Second".into()).await.unwrap();
    workspaces.select(second.id).await.unwrap();
    let application = application_with_workspace_ports(ports.clone(), workspaces);

    let error = application
        .socket_capture_clear(second.id, false)
        .await
        .unwrap_err();
    assert_eq!(error.view_model.code, "CONFIRMATION_REQUIRED");
    assert_eq!(ports.socket_capture_clear_calls.load(Ordering::SeqCst), 0);

    let result = application
        .socket_capture_clear(second.id, true)
        .await
        .unwrap();
    assert!(result.success);
    assert_eq!(result.message, "已清空 3 条 Socket 抓包记录。");
    assert_eq!(ports.socket_capture_clear_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        ports.socket_capture_clear_workspaces.lock().as_slice(),
        &[second.id]
    );
    assert_ne!(first.id, second.id);
}

#[tokio::test]
async fn t28_socket_clear_rejects_a_workspace_that_is_no_longer_selected() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let first = workspaces.list().await.unwrap().remove(0);
    let second = workspaces.create("Second".into()).await.unwrap();
    workspaces.select(second.id).await.unwrap();
    let application = application_with_workspace_ports(ports.clone(), workspaces);

    // UI 对 first 完成确认后，Workspace 已经切换为 second。Application 必须以确认时的
    // first.id 重新校验选择状态，不能把删除目标静默替换成当前选中的 second。
    let error = application
        .socket_capture_clear(first.id, true)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "WORKSPACE_SELECTION_CHANGED");
    assert_eq!(error.view_model.entity_id, Some(first.id.to_string()));
    assert_eq!(ports.socket_capture_clear_calls.load(Ordering::SeqCst), 0);
    assert!(ports.socket_capture_clear_workspaces.lock().is_empty());
}
