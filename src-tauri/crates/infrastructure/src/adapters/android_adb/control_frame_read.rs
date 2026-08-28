use std::time::Duration;

use intercept_proxy_application::{ANDROID_CONTROL_MAX_FRAME_BYTES, AppError, AppResult};
use tokio::io::{AsyncRead, AsyncReadExt};

pub(super) async fn read_control_response_frame(
    reader: &mut (impl AsyncRead + Unpin),
    deadline: Duration,
) -> AppResult<Vec<u8>> {
    tokio::time::timeout(deadline, async {
        let mut prefix = [0_u8; 4];
        reader.read_exact(&mut prefix).await.map_err(|error| {
            AppError::new(
                "ANDROID_CONTROL_SOCKET_FAILED",
                format!("读取设备端控制响应失败：{error}"),
            )
        })?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length > ANDROID_CONTROL_MAX_FRAME_BYTES {
            return Err(AppError::new(
                "ANDROID_PROTOCOL_FRAME_TOO_LARGE",
                "设备端响应超过 1 MiB 上限。",
            ));
        }
        let mut payload = vec![0; length];
        reader.read_exact(&mut payload).await.map_err(|error| {
            AppError::new(
                "ANDROID_CONTROL_SOCKET_FAILED",
                format!("设备端响应被截断：{error}"),
            )
        })?;
        Ok(payload)
    })
    .await
    .map_err(|_| {
        AppError::new(
            "ANDROID_CONTROL_SOCKET_TIMEOUT",
            "读取设备端完整控制响应超时。",
        )
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn payload_read_has_a_complete_deadline_after_prefix_arrives() {
        let (mut reader, mut writer) = tokio::io::duplex(64);
        let writer_task = tokio::spawn(async move {
            writer.write_all(&8_u32.to_be_bytes()).await.unwrap();
            writer.write_all(b"x").await.unwrap();
            std::future::pending::<()>().await;
        });

        let error = read_control_response_frame(&mut reader, Duration::from_millis(25))
            .await
            .expect_err("partial payload must not wait forever");

        assert_eq!(error.view_model.code, "ANDROID_CONTROL_SOCKET_TIMEOUT");
        writer_task.abort();
    }
}
