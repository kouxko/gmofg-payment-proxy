use super::{
    AppError, AppResult, ListenerId, ListenerRuntimeState, ListenerStatusViewModel, ProxyListener,
    ProxyWorkspace, SocketAddr, TcpListener, UiTone,
};

pub(super) fn upstream_tls_test_error(
    listener_id: ListenerId,
    error: &intercept_proxy_runtime::ProxyError,
) -> AppError {
    // rustls 会把“当前信任库里没有签发者”作为 UnknownIssuer 返回。这里不能自动
    // 回退到系统信任根，否则用户显式选择的私有 CA 会被静默绕过；应当把根因和
    // 两种正确处理方式明确交给 UI。
    if error.code == "TLS_HANDSHAKE_FAILED" && error.message.contains("UnknownIssuer") {
        return AppError::new(
            error.code,
            "上游 Server 证书不受当前 CA 信任。当前选择的 CA 不是该 Server 证书链的签发者。",
        )
        .entity(listener_id.to_string())
        .retryable(
            "公开 HTTPS 请选择“使用操作系统信任根”；私有 Server 请导入其真实签发 CA 后重试。",
        );
    }
    let message = match error.code {
        "CONFIG_INVALID" => format!("上游地址配置无效：{}", error.message),
        "CERTIFICATE_NOT_READY" | "CERTIFICATE_INVALID" => {
            format!("上游证书配置无效：{}", error.message)
        }
        "TLS_HANDSHAKE_FAILED" => format!("上游 Server TLS 握手失败：{}", error.message),
        "UPSTREAM_CONNECT_TIMEOUT" => format!("连接上游 Server 超时：{}", error.message),
        "IO_ERROR" => format!("无法连接上游 Server：{}", error.message),
        _ => format!("上游 TLS 测试失败：{}", error.message),
    };
    let error = AppError::new(error.code, message).entity(listener_id.to_string());
    if matches!(
        error.view_model.code.as_str(),
        "TLS_HANDSHAKE_FAILED" | "UPSTREAM_CONNECT_TIMEOUT" | "IO_ERROR"
    ) {
        error.retryable("检查 Server 地址、网络、CA、主机名和可选客户端证书后重试。")
    } else {
        error
    }
}

pub(super) fn ensure_snapshot_matches(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
) -> AppResult<()> {
    let persisted = workspace
        .listeners
        .iter()
        .find(|candidate| candidate.id == listener.id)
        .ok_or_else(|| {
            AppError::new("LISTENER_NOT_FOUND", "Workspace 中不存在该 Listener。")
                .entity(listener.id.to_string())
        })?;
    if persisted == listener {
        Ok(())
    } else {
        Err(AppError::new(
            "REVISION_CONFLICT",
            "Listener 配置与当前 Workspace 快照不一致，请重新加载。",
        )
        .entity(listener.id.to_string()))
    }
}

pub(super) fn running_status(
    listener_id: ListenerId,
    listen_address: String,
) -> ListenerStatusViewModel {
    ListenerStatusViewModel {
        listener_id,
        state: ListenerRuntimeState::Running,
        state_text: "运行中".into(),
        ui_tone: UiTone::Positive,
        listen_address,
        fault_reason: None,
        can_start: false,
        can_stop: true,
        active_connections: 0,
        client_to_server_bytes: 0,
        server_to_client_bytes: 0,
        retained_diagnostic_evictions: 0,
    }
}

pub(super) fn parse_bind_address(
    address: &str,
    port: u16,
    id: ListenerId,
) -> AppResult<SocketAddr> {
    format!("{address}:{port}")
        .parse::<SocketAddr>()
        .map_err(|error| {
            AppError::new("CONFIG_INVALID", format!("Listener 地址无法解析：{error}"))
                .entity(id.to_string())
        })
}

pub(super) async fn bind_tcp_listener(
    address: SocketAddr,
    id: ListenerId,
) -> AppResult<TcpListener> {
    TcpListener::bind(address).await.map_err(|error| {
        AppError::new(
            if error.kind() == std::io::ErrorKind::AddrInUse {
                "PORT_IN_USE"
            } else {
                "LISTENER_START_FAILED"
            },
            format!("无法监听 {address}：{error}"),
        )
        .entity(id.to_string())
    })
}
