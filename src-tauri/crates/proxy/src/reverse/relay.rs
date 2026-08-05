use super::{
    AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BoxIo, CancellationToken, Duration,
    ErrorCode, ProxyError, Result,
};

pub(super) async fn relay_exact(
    downstream: BoxIo,
    upstream: BoxIo,
    read_timeout: Duration,
    write_timeout: Duration,
    cancellation: CancellationToken,
) -> Result<()> {
    let (down_read, down_write) = tokio::io::split(downstream);
    let (up_read, up_write) = tokio::io::split(upstream);
    let upstream_direction = copy_exact_direction(
        down_read,
        up_write,
        read_timeout,
        write_timeout,
        cancellation.child_token(),
    );
    let downstream_direction = copy_exact_direction(
        up_read,
        down_write,
        read_timeout,
        write_timeout,
        cancellation.child_token(),
    );
    tokio::try_join!(upstream_direction, downstream_direction)?;
    Ok(())
}

async fn copy_exact_direction<R, W>(
    mut reader: R,
    mut writer: W,
    read_timeout: Duration,
    write_timeout: Duration,
    cancellation: CancellationToken,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0_u8; 16 * 1024];
    loop {
        let read = timeout_cancel(
            read_timeout,
            &cancellation,
            reader.read(&mut buffer),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("read reverse stream", &error))?;
        if read == 0 {
            writer
                .shutdown()
                .await
                .map_err(|error| ProxyError::io("half-close reverse stream", &error))?;
            return Ok(());
        }
        timeout_cancel(
            write_timeout,
            &cancellation,
            writer.write_all(&buffer[..read]),
            ErrorCode::UpstreamWriteTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("write reverse stream", &error))?;
        timeout_cancel(
            write_timeout,
            &cancellation,
            writer.flush(),
            ErrorCode::UpstreamWriteTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("flush reverse stream", &error))?;
    }
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
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(ProxyError::new(ErrorCode::ProxyStopped, "reverse listener stopped")),
        outcome = tokio::time::timeout(duration, future) => outcome.map_err(|_| ProxyError::new(timeout_code, format!("reverse I/O timed out after {} ms", duration.as_millis()))),
    }
}
