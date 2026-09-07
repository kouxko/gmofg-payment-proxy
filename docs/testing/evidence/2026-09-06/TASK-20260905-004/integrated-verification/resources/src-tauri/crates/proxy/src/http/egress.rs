//! Shared HTTP/1 egress after target selection; adapters own TCP/TLS and header policy.

use super::{
    AsyncWriteExt, BodyExt, BoxIo, CancellationToken, Duration, ErrorCode, FaultAction,
    ForwardRequest, Future, Http1ExchangeConfig, InformationalResponseSink, InjectedTimeoutStage,
    Message, MessageLimits, PacedBody, PacedBodyError, ProxyError, Result, TrafficDirection,
    TrafficSchedule, UpstreamExchange, UpstreamWriteReceipt, injected_timeout, message_wire_head,
    send_http1_request, timeout_stage, traffic_schedule, wait_for_injected_timeout,
};

pub(crate) struct Http1TransportSettings {
    pub(crate) write_timeout: Duration,
    pub(crate) read_timeout: Duration,
    pub(crate) limits: MessageLimits,
}

pub(crate) async fn send_http1_exchange(
    connect: impl Future<Output = Result<BoxIo>>,
    request: ForwardRequest,
    actions: &[FaultAction],
    settings: Http1TransportSettings,
    informational: Option<&InformationalResponseSink>,
    write_receipt: &UpstreamWriteReceipt,
    cancellation: &CancellationToken,
) -> Result<UpstreamExchange> {
    wait_for_injected_timeout(actions, InjectedTimeoutStage::Connect, cancellation).await?;
    let mut io = connect.await?;
    wait_for_injected_timeout(actions, InjectedTimeoutStage::Write, cancellation).await?;
    let schedule = traffic_schedule(actions, TrafficDirection::Upstream)?;
    let close_after_write = actions.iter().any(|action| {
        matches!(
            action,
            FaultAction::DropResponse {
                read_upstream: false
            }
        )
    });
    let inject_read_timeout = injected_timeout(actions, InjectedTimeoutStage::Read).is_some();
    if schedule.disconnect_after_bytes.is_some() || close_after_write || inject_read_timeout {
        write_scheduled_request(
            &mut io,
            &request.message,
            schedule,
            settings.write_timeout,
            cancellation,
        )
        .await?;
        write_receipt.confirm_flushed();
        if close_after_write {
            timeout_stage(
                settings.write_timeout,
                cancellation,
                io.shutdown(),
                ErrorCode::UpstreamWriteTimeout,
            )
            .await?
            .map_err(|error| ProxyError::io("close injected upstream request", &error))?;
            return Err(ProxyError::new(
                ErrorCode::ClientDisconnected,
                "upstream request intentionally closed after complete write",
            ));
        }
        wait_for_injected_timeout(actions, InjectedTimeoutStage::Read, cancellation).await?;
        return Err(ProxyError::new(
            ErrorCode::Internal,
            "injected request completed without its terminal action",
        ));
    }
    send_http1_request(
        io,
        request,
        Http1ExchangeConfig {
            schedule,
            write_timeout: settings.write_timeout,
            read_timeout: settings.read_timeout,
            limits: settings.limits,
        },
        informational,
        write_receipt,
        cancellation,
    )
    .await
}

async fn write_scheduled_request(
    io: &mut BoxIo,
    message: &Message,
    schedule: TrafficSchedule,
    write_timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<()> {
    if schedule
        .disconnect_after_bytes
        .is_some_and(|offset| offset >= message.body.len())
    {
        return Err(ProxyError::new(
            ErrorCode::ConfigInvalid,
            "upstream disconnect offset must be smaller than request body",
        ));
    }
    let head = message_wire_head(message)?;
    timeout_stage(
        write_timeout,
        cancellation,
        io.write_all(&head),
        ErrorCode::UpstreamWriteTimeout,
    )
    .await?
    .map_err(|error| ProxyError::io("write upstream request headers", &error))?;
    let mut body = PacedBody::new(
        message.body.clone(),
        message.body.len(),
        schedule,
        cancellation.clone(),
    );
    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => {
                if let Ok(data) = frame.into_data() {
                    timeout_stage(
                        write_timeout,
                        cancellation,
                        io.write_all(&data),
                        ErrorCode::UpstreamWriteTimeout,
                    )
                    .await?
                    .map_err(|error| ProxyError::io("write paced upstream body", &error))?;
                }
            }
            Err(PacedBodyError::Disconnected) => {
                let _ = io.shutdown().await;
                return Err(ProxyError::new(
                    ErrorCode::FaultStreamAborted,
                    "request intentionally disconnected during upstream write",
                ));
            }
            Err(PacedBodyError::Cancelled) => {
                return Err(ProxyError::new(
                    ErrorCode::FaultExecutionCancelled,
                    "weak-network request cancelled",
                ));
            }
        }
    }
    timeout_stage(
        write_timeout,
        cancellation,
        io.flush(),
        ErrorCode::UpstreamWriteTimeout,
    )
    .await?
    .map_err(|error| ProxyError::io("flush upstream request", &error))
}
