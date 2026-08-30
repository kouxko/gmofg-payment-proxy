//! 协议与方向强类型化的完整读写 Pipeline。

use std::{fmt, marker::PhantomData};

use async_trait::async_trait;

use crate::{
    Decode, Direction, Display, Encode, Envelope, Error, Frame, FrameResult, Http, Protocol,
    Reader, Rules, Socket, SocketContext, Writer,
};
use crate::{ObservedProtocol, observation};

/// Exchange 只持有 upstream/downstream 两个 `Pipeline<P, D>`。
/// Read/Write 的不同阶段集合封装在内部 trait object，不泄漏额外 role 泛型。
pub struct Pipeline<P: Protocol, D: Direction> {
    reader: Box<dyn ReadPipeline<P, D>>,
    writer: Box<dyn WritePipeline<P, D>>,
    marker: PhantomData<fn() -> (P, D)>,
}

impl<P: Protocol, D: Direction> fmt::Debug for Pipeline<P, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Pipeline")
            .field("protocol", &std::any::type_name::<P>())
            .field("direction", &D::KIND)
            .finish_non_exhaustive()
    }
}

impl<P: ObservedProtocol, D: Direction> Pipeline<P, D> {
    pub fn new(reader: Box<dyn ReadPipeline<P, D>>, writer: Box<dyn WritePipeline<P, D>>) -> Self {
        Self {
            reader,
            writer,
            marker: PhantomData,
        }
    }

    pub async fn read(
        &mut self,
        reader: &mut dyn Reader<P, D>,
    ) -> Result<Option<Envelope<P, D>>, Error> {
        self.reader.read(reader).await
    }

    pub async fn write(
        &mut self,
        writer: &mut dyn Writer<P, D>,
        envelope: &Envelope<P, D>,
    ) -> Result<P::Context, Error> {
        self.writer.write(writer, envelope).await
    }
}

#[async_trait]
pub trait ReadPipeline<P: Protocol, D: Direction>: Send {
    async fn read(
        &mut self,
        reader: &mut dyn Reader<P, D>,
    ) -> Result<Option<Envelope<P, D>>, Error>;
}

#[async_trait]
pub trait WritePipeline<P: Protocol, D: Direction>: Send {
    async fn write(
        &mut self,
        writer: &mut dyn Writer<P, D>,
        envelope: &Envelope<P, D>,
    ) -> Result<P::Context, Error>;
}

/// HTTP Reader 固定执行 Decode -> Display。
pub struct HttpRead<D: Direction> {
    decode: Box<dyn Decode<Http, D>>,
    display: Box<dyn Display>,
}

impl<D: Direction> fmt::Debug for HttpRead<D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("HttpRead").finish_non_exhaustive()
    }
}

impl<D: Direction> HttpRead<D> {
    pub fn new(decode: Box<dyn Decode<Http, D>>, display: Box<dyn Display>) -> Self {
        Self { decode, display }
    }
}

#[async_trait]
impl<D: Direction> ReadPipeline<Http, D> for HttpRead<D> {
    async fn read(
        &mut self,
        reader: &mut dyn Reader<Http, D>,
    ) -> Result<Option<Envelope<Http, D>>, Error> {
        let Some(context) = reader.read().await.inspect_err(|error| {
            observation::failed::<D>("read", error);
        })?
        else {
            return Ok(None);
        };

        tracing::debug!(
            target: "intercept_proxy::exchange::diagnostic",
            direction = ?D::KIND,
            header = %context.header,
            body = %context.body,
            "HTTP context read"
        );
        let document = self.decode.decode(&context).await.inspect_err(|error| {
            observation::failed_with_context::<Http, D>("decode", &context, error);
        })?;
        let display = match self.display.display(&document).await {
            Ok(display) => display,
            Err(error) => {
                observation::failed_with_context::<Http, D>("display", &context, &error);
                tracing::warn!(
                    target: "intercept_proxy::exchange::diagnostic",
                    direction = ?D::KIND,
                    error = %error,
                    "Display failed; using HTTP body"
                );
                context.body.clone()
            }
        };
        Ok(Some(Envelope {
            context,
            document,
            display,
            direction: PhantomData,
        }))
    }
}

/// Socket Reader 在本次 read 调用内部累计 chunk，Frame 完整后立即返回一个 Envelope。
pub struct SocketRead<D: Direction> {
    frame: Box<dyn Frame<D>>,
    decode: Box<dyn Decode<Socket, D>>,
    display: Box<dyn Display>,
}

impl<D: Direction> fmt::Debug for SocketRead<D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("SocketRead").finish_non_exhaustive()
    }
}

impl<D: Direction> SocketRead<D> {
    pub fn new(
        frame: Box<dyn Frame<D>>,
        decode: Box<dyn Decode<Socket, D>>,
        display: Box<dyn Display>,
    ) -> Self {
        Self {
            frame,
            decode,
            display,
        }
    }
}

#[async_trait]
impl<D: Direction> ReadPipeline<Socket, D> for SocketRead<D> {
    async fn read(
        &mut self,
        reader: &mut dyn Reader<Socket, D>,
    ) -> Result<Option<Envelope<Socket, D>>, Error> {
        let mut buffer = Vec::new();
        loop {
            let Some(chunk) = reader.read().await.inspect_err(|error| {
                observation::failed::<D>("read", error);
            })?
            else {
                return if buffer.is_empty() {
                    Ok(None)
                } else {
                    Err(failure_with_context::<D>(
                        "frame",
                        "Socket closed before a complete Frame",
                        &buffer,
                    ))
                };
            };
            if chunk.data.is_empty() {
                return Err(failure::<D>("read", "Socket returned an empty chunk"));
            }

            tracing::debug!(
                target: "intercept_proxy::exchange::diagnostic",
                direction = ?D::KIND,
                data = ?chunk.data,
                "Socket chunk read"
            );
            buffer.extend_from_slice(&chunk.data);
            match self.frame.split(&buffer).await.inspect_err(|error| {
                observation::failed_with_context::<Socket, D>(
                    "frame",
                    &SocketContext {
                        data: buffer.clone(),
                    },
                    error,
                );
            })? {
                FrameResult::NeedMore => tracing::debug!(
                    target: "intercept_proxy::exchange::diagnostic",
                    direction = ?D::KIND,
                    buffered = buffer.len(),
                    "Frame needs more data"
                ),
                FrameResult::Complete { consumed } => {
                    if consumed == 0 || consumed > buffer.len() {
                        return Err(failure_with_context::<D>(
                            "frame",
                            "Frame returned invalid consumed bytes",
                            &buffer,
                        ));
                    }
                    // 协议模式不支持 pipelining；同一次 read 出现第二个 Frame 或尾部数据
                    // 是 Endpoint 合同错误，不能留给下一笔造成错配。
                    if consumed != buffer.len() {
                        return Err(failure_with_context::<D>(
                            "protocol",
                            "Socket read contained data beyond one complete Frame",
                            &buffer,
                        ));
                    }
                    return self.complete(buffer).await.map(Some);
                }
            }
        }
    }
}

impl<D: Direction> SocketRead<D> {
    async fn complete(&mut self, data: Vec<u8>) -> Result<Envelope<Socket, D>, Error> {
        let context = SocketContext { data };
        let document = self.decode.decode(&context).await.inspect_err(|error| {
            observation::failed_with_context::<Socket, D>("decode", &context, error);
        })?;
        let display = match self.display.display(&document).await {
            Ok(display) => display,
            Err(error) => {
                observation::failed_with_context::<Socket, D>("display", &context, &error);
                tracing::warn!(
                    target: "intercept_proxy::exchange::diagnostic",
                    direction = ?D::KIND,
                    error = %error,
                    "Display failed; using Socket hex"
                );
                hex(&context.data)
            }
        };
        Ok(Envelope {
            context,
            document,
            display,
            direction: PhantomData,
        })
    }
}

/// HTTP 与 Socket Writer 都固定执行 Rules -> Encode -> transport Writer。
pub struct Write<P: Protocol, D: Direction> {
    rules: Box<dyn Rules>,
    encode: Box<dyn Encode<P, D>>,
}

impl<P: Protocol, D: Direction> fmt::Debug for Write<P, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Write").finish_non_exhaustive()
    }
}

impl<P: Protocol, D: Direction> Write<P, D> {
    pub fn new(rules: Box<dyn Rules>, encode: Box<dyn Encode<P, D>>) -> Self {
        Self { rules, encode }
    }
}

#[async_trait]
impl<P: ObservedProtocol, D: Direction> WritePipeline<P, D> for Write<P, D> {
    async fn write(
        &mut self,
        writer: &mut dyn Writer<P, D>,
        envelope: &Envelope<P, D>,
    ) -> Result<P::Context, Error> {
        let document = self
            .rules
            .apply(envelope.document().clone())
            .await
            .inspect_err(|error| {
                observation::failed_with_context::<P, D>("rules", envelope.context(), error);
            })?;
        let context = self
            .encode
            .encode(envelope.context(), &document)
            .await
            .inspect_err(|error| {
                observation::failed_with_context::<P, D>("encode", envelope.context(), error);
            })?;

        writer.write(context.clone()).await.inspect_err(|error| {
            observation::failed_with_context::<P, D>("write", &context, error);
        })
    }
}

fn hex(data: &[u8]) -> String {
    data.iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn failure<D: Direction>(stage: &'static str, message: &'static str) -> Error {
    let error = Error::new(message);
    observation::failed::<D>(stage, &error);
    error
}

fn failure_with_context<D: Direction>(
    stage: &'static str,
    message: &'static str,
    bytes: &[u8],
) -> Error {
    let error = Error::new(message);
    observation::failed_with_context::<Socket, D>(
        stage,
        &SocketContext {
            data: bytes.to_vec(),
        },
        &error,
    );
    error
}
