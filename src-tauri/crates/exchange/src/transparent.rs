//! Socket 透明模式的原始双向数据通路。

use std::fmt;

use async_trait::async_trait;

use crate::{Direction, Downstream, Error, Upstream, observation};

#[async_trait]
pub trait RawReader: Send {
    /// `Some` 必须是一次非空真实 read；`None` 表示该方向 EOF。
    async fn read(&mut self) -> Result<Option<Vec<u8>>, Error>;
}

#[async_trait]
pub trait RawWriter: Send {
    /// 必须循环处理底层 partial write，完整写出整个 slice；发生错误后不得重新发送业务字节。
    async fn write(&mut self, bytes: &[u8]) -> Result<(), Error>;

    /// 只关闭当前写半边，把 EOF 传播给对端；另一读方向继续工作。
    async fn finish(&mut self) -> Result<(), Error>;
}

/// 进入透明 relay 时拆成独立读写 half。
/// 所有实现必须保证两个 half 全部 Drop 后底层 TCP/TLS 连接关闭。
pub trait RawConnection: Send {
    fn into_split(self: Box<Self>) -> (Box<dyn RawReader>, Box<dyn RawWriter>);
}

#[async_trait]
pub trait RawServer: Send {
    /// 第一段 App bytes 已经读取后才调用 connect，Endpoint 在 Exchange 创建时已经固定。
    async fn connect(&mut self, first_app_bytes: &[u8]) -> Result<Box<dyn RawConnection>, Error>;
}

/// 透明模式不创建 Envelope，也不调用任何协议 Pipeline 能力。
pub struct TransparentExchange {
    app: Box<dyn RawConnection>,
    server: Box<dyn RawServer>,
}

impl fmt::Debug for TransparentExchange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransparentExchange")
            .finish_non_exhaustive()
    }
}

impl TransparentExchange {
    pub fn new(app: Box<dyn RawConnection>, server: Box<dyn RawServer>) -> Self {
        Self { app, server }
    }

    pub async fn exchange(mut self) -> Result<(), Error> {
        let (mut app_reader, app_writer) = self.app.into_split();
        let Some(first) = read_chunk::<Upstream>(&mut *app_reader).await? else {
            // App 未发送数据就关闭时不建立 Server connection。
            return Ok(());
        };

        let server = self.server.connect(&first).await.inspect_err(|error| {
            observation::raw_failed::<Upstream>("connect", Some(&first), error);
        })?;
        let (server_reader, mut server_writer) = server.into_split();

        // 首段必须先原样完整写出，之后才能并发推动两个方向。
        write_chunk::<Upstream>(&mut *server_writer, &first).await?;
        tokio::try_join!(
            relay::<Upstream>(app_reader, server_writer),
            relay::<Downstream>(server_reader, app_writer),
        )?;
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

    // 单向 EOF 只传播写半关闭；try_join 让另一方向继续到自身 EOF 或失败。
    writer.finish().await.inspect_err(|error| {
        observation::raw_failed::<D>("half_close", None, error);
    })
}

async fn read_chunk<D: Direction>(reader: &mut dyn RawReader) -> Result<Option<Vec<u8>>, Error> {
    let bytes = reader.read().await.inspect_err(|error| {
        observation::raw_failed::<D>("read", None, error);
    })?;
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    if bytes.is_empty() {
        let error = Error::new("RawReader returned an empty chunk");
        observation::raw_failed::<D>("read", Some(&bytes), &error);
        return Err(error);
    }

    observation::raw_received::<D>(&bytes);
    Ok(Some(bytes))
}

async fn write_chunk<D: Direction>(writer: &mut dyn RawWriter, bytes: &[u8]) -> Result<(), Error> {
    writer.write(bytes).await.inspect_err(|error| {
        observation::raw_failed::<D>("write", Some(bytes), error);
    })?;

    // 只有完整 write 成功才能产生 sent。
    observation::raw_sent::<D>(bytes);
    Ok(())
}
