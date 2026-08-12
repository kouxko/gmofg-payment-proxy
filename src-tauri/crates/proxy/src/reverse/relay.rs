use super::{BoxIo, CancellationToken, Duration, ErrorCode, Result};
use crate::transport::relay::{
    RelayTimeoutCodes, RelayTimeouts, relay_bidirectional, timeout_cancel_first,
};

pub(super) async fn relay_exact(
    downstream: BoxIo,
    upstream: BoxIo,
    read_timeout: Duration,
    write_timeout: Duration,
    cancellation: CancellationToken,
) -> Result<()> {
    relay_bidirectional(
        downstream,
        upstream,
        RelayTimeouts::new(read_timeout, write_timeout, RelayTimeoutCodes::upstream()),
        cancellation,
    )
    .await
    .map(|_| ())
    .map_err(|failure| {
        tracing::debug!(
            direction = ?failure.direction,
            operation = ?failure.operation,
            client_to_server_bytes = failure.bytes.client_to_server,
            server_to_client_bytes = failure.bytes.server_to_client,
            code = failure.error.code,
            "reverse relay failed"
        );
        failure.error
    })
}

pub(super) async fn timeout_cancel<F, T>(
    duration: Duration,
    cancellation: &CancellationToken,
    future: F,
    timeout_code: ErrorCode,
) -> Result<T>
where
    F: std::future::Future<Output = T>,
{
    timeout_cancel_first(
        duration,
        cancellation,
        future,
        timeout_code,
        "reverse listener stopped",
        "reverse I/O",
    )
    .await
}
