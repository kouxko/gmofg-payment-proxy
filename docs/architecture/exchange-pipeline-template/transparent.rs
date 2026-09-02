//! Socket 透明转发设计模板。本文件不参与当前工程编译。

use async_trait::async_trait;

use super::core::{Direction, Downstream, Error, Upstream};

/// 透明模式只读取操作系统实际交付的原始字节，不识别协议消息。
#[async_trait]
pub trait RawReader: Send {
    /// `Some(bytes)` 是一次非空原始读取；`None` 是该方向 EOF。
    async fn read(&mut self) -> Result<Option<Vec<u8>>, Error>;
}

/// 透明模式只写原始字节，不执行 Frame/Decode/Rules/Encode。
#[async_trait]
pub trait RawWriter: Send {
    /// 必须完整写出整个 slice；不得变换、合并或补充字节。
    async fn write(&mut self, bytes: &[u8]) -> Result<(), Error>;

    /// 只关闭当前写半边，把 EOF 传播给对端；读半边仍可继续接收。
    ///
    /// 这是透明 TCP 转发的内部能力，不属于协议模式的 `Writer<P, D>`。
    async fn finish(&mut self) -> Result<(), Error>;
}

/// 原始全双工连接只在进入透明转发循环时拆成独立的读写半边。
/// 所有实现必须保证：两个 half 全部释放后，底层 TCP/TLS 连接随之关闭。
/// relay 失败会取消另一方向并释放全部 half，因此不再增加独立 RawCloser。
pub trait RawConnection: Send {
    fn into_split(self: Box<Self>) -> (Box<dyn RawReader>, Box<dyn RawWriter>);
}

/// RemoteRawServer 建立真实 TCP/TLS 连接；LocalRawServer 建立进程内 Echo 连接。
/// 两者对 TransparentExchange 暴露完全相同的接口。
#[async_trait]
pub trait RawServer: Send {
    /// 收到第一段 App 数据后才连接 Server，避免空连接占用后台资源。
    async fn connect(&mut self, first_app_bytes: &[u8]) -> Result<Box<dyn RawConnection>, Error>;
}

/// 透明模式不是 Pipeline 的特殊实现，而是同一个 Exchange 下的原始数据通路。
pub struct TransparentExchange {
    app: Box<dyn RawConnection>,
    server: Box<dyn RawServer>,
}

impl TransparentExchange {
    pub fn new(app: Box<dyn RawConnection>, server: Box<dyn RawServer>) -> Self {
        Self { app, server }
    }

    pub async fn exchange(mut self) -> Result<(), Error> {
        let (mut app_reader, app_writer) = self.app.into_split();

        // App 未发送任何数据就断开时，不创建 Server 连接。
        let Some(first_app_bytes) = read_chunk::<Upstream>(&mut *app_reader).await? else {
            return Ok(());
        };

        let server_connection = self
            .server
            .connect(&first_app_bytes)
            .await
            .map_err(|error| {
                trace_failure::<Upstream>("connect", Some(&first_app_bytes), &error);
                error
            })?;
        let (server_reader, mut server_writer) = server_connection.into_split();

        // 首段数据必须先原样写出，再进入并发双向 relay。
        write_chunk::<Upstream>(&mut *server_writer, &first_app_bytes).await?;

        tokio::try_join!(
            relay::<Upstream>(app_reader, server_writer),
            relay::<Downstream>(server_reader, app_writer),
        )?;
        // 任一 relay 失败时 try_join 取消另一方向；全部 half 随 future 释放，
        // RawConnection 的 Drop 合同负责关闭底层连接。
        Ok(())
    }
}

async fn relay<D: Direction>(
    mut reader: Box<dyn RawReader>,
    mut writer: Box<dyn RawWriter>,
) -> Result<(), Error> {
    while let Some(bytes) = read_chunk::<D>(&mut *reader).await? {
        write_chunk::<D>(&mut *writer, &bytes).await?;
    }

    // 一侧 EOF 只传播写半关闭，另一方向仍然可以读完剩余响应。
    writer.finish().await.map_err(|error| {
        trace_failure::<D>("half_close", None, &error);
        error
    })
}

async fn read_chunk<D: Direction>(reader: &mut dyn RawReader) -> Result<Option<Vec<u8>>, Error> {
    let bytes = reader.read().await.map_err(|error| {
        trace_failure::<D>("read", None, &error);
        error
    })?;
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    if bytes.is_empty() {
        let error = Error::new("RawReader returned an empty chunk");
        trace_failure::<D>("read", Some(&bytes), &error);
        return Err(error);
    }

    tracing::info!(
        target: "intercept_proxy::exchange::ui",
        event = "received",
        direction = ?D::KIND,
        bytes = ?bytes
    );
    Ok(Some(bytes))
}

async fn write_chunk<D: Direction>(writer: &mut dyn RawWriter, bytes: &[u8]) -> Result<(), Error> {
    writer.write(bytes).await.map_err(|error| {
        trace_failure::<D>("write", Some(bytes), &error);
        error
    })?;

    // sent 必须晚于完整 write 成功。
    tracing::info!(
        target: "intercept_proxy::exchange::ui",
        event = "sent",
        direction = ?D::KIND,
        bytes = ?bytes
    );
    Ok(())
}

fn trace_failure<D: Direction>(stage: &'static str, bytes: Option<&[u8]>, error: &Error) {
    tracing::error!(
        target: "intercept_proxy::exchange::ui",
        event = "failed",
        direction = ?D::KIND,
        stage = stage,
        bytes = ?bytes,
        error = %error.message
    );
}

// LocalRawServer 的推荐实现：
// 1. connect() 创建一对进程内全双工连接；
// 2. 后台任务每次读到非空原始 chunk 后立即把完全相同的字节写回；
// 3. TransparentExchange 使用另一端，流程与 RemoteRawServer 完全一致。
// 它不等待 Frame、EOF 或空闲超时，不解析、不累计、不修改；
// 只保证字节流一致，不保证对端 read 的 chunk 边界一致。
// 它不是旁路 responder，也不调用协议 Pipeline。
