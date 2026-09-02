//! Exchange / Pipeline 核心设计模板。本文件不参与当前工程编译。

use std::{fmt::Debug, marker::PhantomData};

use async_trait::async_trait;

#[derive(Clone, Debug)]
pub struct Document;

pub trait Protocol: Send + Sync + 'static {
    type Context: Clone + Debug + Send + Sync + 'static;
}

pub struct Http;
pub struct Socket;

#[derive(Clone, Debug)]
pub struct HttpContext {
    pub header: String,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct SocketContext {
    pub data: Vec<u8>,
}

impl Protocol for Http {
    type Context = HttpContext;
}

impl Protocol for Socket {
    type Context = SocketContext;
}

pub trait Direction: Send + Sync + 'static {
    const KIND: DirectionKind;
}

pub struct Upstream;
pub struct Downstream;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectionKind {
    Upstream,
    Downstream,
}

impl Direction for Upstream {
    const KIND: DirectionKind = DirectionKind::Upstream;
}

impl Direction for Downstream {
    const KIND: DirectionKind = DirectionKind::Downstream;
}

#[derive(Clone, Debug)]
pub struct Error {
    pub message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[async_trait]
pub trait Reader<P, D>: Send
where
    P: Protocol,
    D: Direction,
{
    async fn read(&mut self) -> Result<Option<P::Context>, Error>;
}

#[async_trait]
pub trait Writer<P, D>: Send
where
    P: Protocol,
    D: Direction,
{
    /// 只有完整 write + flush 成功才能返回 Ok。
    /// 具体实现必须在真实 write/flush 失败位置记录 tracing 后再返回 Err。
    async fn write(&mut self, context: P::Context) -> Result<(), Error>;
}

#[async_trait]
pub trait Frame<D>: Send
where
    D: Direction,
{
    async fn split(&mut self, buffer: &[u8]) -> Result<FrameResult, Error>;
}

pub enum FrameResult {
    /// 当前缓冲区不足以确认完整 Frame。
    ///
    /// 不携带期望字节数：并非所有协议都能提前得知精确长度，当前 Reader 也不提供
    /// `read_exact`/`read_at_least` 语义。Pipeline 收到该结果后继续执行普通 read。
    NeedMore,
    Complete {
        consumed: usize,
    },
}

#[async_trait]
pub trait Decode<P, D>: Send
where
    P: Protocol,
    D: Direction,
{
    async fn decode(&mut self, context: &P::Context) -> Result<Document, Error>;
}

#[async_trait]
pub trait Display: Send {
    /// 直接返回给 UI 显示的文本/HTML。
    async fn display(&mut self, document: &Document) -> Result<String, Error>;
}

#[async_trait]
pub trait Rules: Send {
    async fn apply(&mut self, document: Document) -> Result<Document, Error>;
}

#[async_trait]
pub trait Encode<P, D>: Send
where
    P: Protocol,
    D: Direction,
{
    async fn encode(
        &mut self,
        original: &P::Context,
        document: &Document,
    ) -> Result<P::Context, Error>;
}

pub struct Envelope<P, D>
where
    P: Protocol,
    D: Direction,
{
    context: P::Context,
    document: Document,
    display: String,
    direction: PhantomData<D>,
}

impl<P, D> Envelope<P, D>
where
    P: Protocol,
    D: Direction,
{
    pub fn context(&self) -> &P::Context {
        &self.context
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn display(&self) -> &str {
        &self.display
    }
}

/// Exchange 直接持有 upstream/downstream 两个 Pipeline，不定义 Flow。
pub struct Pipeline<P, D>
where
    P: Protocol,
    D: Direction,
{
    reader: Box<dyn ReadPipeline<P, D>>,
    writer: Box<dyn WritePipeline<P, D>>,
    protocol: PhantomData<P>,
    direction: PhantomData<D>,
}

pub struct HttpRead<D: Direction> {
    decode: Box<dyn Decode<Http, D>>,
    display: Box<dyn Display>,
}

pub struct SocketRead<D: Direction> {
    frame: Box<dyn Frame<D>>,
    decode: Box<dyn Decode<Socket, D>>,
    display: Box<dyn Display>,
}

pub struct Write<P: Protocol, D: Direction> {
    rules: Box<dyn Rules>,
    encode: Box<dyn Encode<P, D>>,
}

impl<D: Direction> HttpRead<D> {
    pub fn new(decode: Box<dyn Decode<Http, D>>, display: Box<dyn Display>) -> Self {
        Self { decode, display }
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

impl<P: Protocol, D: Direction> Write<P, D> {
    pub fn new(rules: Box<dyn Rules>, encode: Box<dyn Encode<P, D>>) -> Self {
        Self { rules, encode }
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

#[async_trait]
impl<D: Direction> ReadPipeline<Http, D> for HttpRead<D> {
    async fn read(
        &mut self,
        reader: &mut dyn Reader<Http, D>,
    ) -> Result<Option<Envelope<Http, D>>, Error> {
        let Some(context) = reader.read().await.map_err(|error| {
            trace_failure::<D>("read", &error);
            error
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
        let document = self.decode.decode(&context).await.map_err(|error| {
            trace_failure::<D>("decode", &error);
            error
        })?;
        let display = match self.display.display(&document).await {
            Ok(display) => display,
            Err(error) => {
                tracing::warn!(
                    target: "intercept_proxy::exchange::diagnostic",
                    direction = ?D::KIND,
                    error = ?error,
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

#[async_trait]
impl<D: Direction> ReadPipeline<Socket, D> for SocketRead<D> {
    async fn read(
        &mut self,
        reader: &mut dyn Reader<Socket, D>,
    ) -> Result<Option<Envelope<Socket, D>>, Error> {
        // 不支持 pipelining，buffer 只属于当前这次 read()。
        let mut buffer = Vec::new();
        loop {
            let Some(chunk) = reader.read().await.map_err(|error| {
                trace_failure::<D>("read", &error);
                error
            })?
            else {
                return if buffer.is_empty() {
                    Ok(None)
                } else {
                    let error = Error::new("Socket closed before a complete Frame");
                    trace_failure::<D>("frame", &error);
                    Err(error)
                };
            };
            if chunk.data.is_empty() {
                let error = Error::new("Socket returned an empty chunk");
                trace_failure::<D>("read", &error);
                return Err(error);
            }
            tracing::debug!(
                target: "intercept_proxy::exchange::diagnostic",
                direction = ?D::KIND,
                data = ?chunk.data,
                "Socket chunk read"
            );
            buffer.extend_from_slice(&chunk.data);
            match self.frame.split(&buffer).await.map_err(|error| {
                trace_failure::<D>("frame", &error);
                error
            })? {
                FrameResult::NeedMore => {
                    tracing::debug!(
                        target: "intercept_proxy::exchange::diagnostic",
                        direction = ?D::KIND,
                        buffered = buffer.len(),
                        "Frame needs more data"
                    );
                    continue;
                }
                FrameResult::Complete { consumed } => {
                    if consumed == 0 || consumed > buffer.len() {
                        let error = Error::new("Frame returned invalid consumed bytes");
                        trace_failure::<D>("frame", &error);
                        return Err(error);
                    }
                    if consumed != buffer.len() {
                        let error =
                            Error::new("Socket read contained data beyond one complete Frame");
                        trace_failure::<D>("protocol", &error);
                        return Err(error);
                    }
                    let context = SocketContext { data: buffer };
                    let document = self.decode.decode(&context).await.map_err(|error| {
                        trace_failure::<D>("decode", &error);
                        error
                    })?;
                    let display = match self.display.display(&document).await {
                        Ok(display) => display,
                        Err(error) => {
                            tracing::warn!(
                                target: "intercept_proxy::exchange::diagnostic",
                                direction = ?D::KIND,
                                error = ?error,
                                "Display failed; using Socket hex"
                            );
                            hex(&context.data)
                        }
                    };
                    return Ok(Some(Envelope {
                        context,
                        document,
                        display,
                        direction: PhantomData,
                    }));
                }
            }
        }
    }
}

#[async_trait]
impl<P: Protocol, D: Direction> WritePipeline<P, D> for Write<P, D> {
    async fn write(
        &mut self,
        writer: &mut dyn Writer<P, D>,
        envelope: &Envelope<P, D>,
    ) -> Result<P::Context, Error> {
        let document = self
            .rules
            .apply(envelope.document().clone())
            .await
            .map_err(|error| {
                trace_failure::<D>("rules", &error);
                error
            })?;
        let context = self
            .encode
            .encode(envelope.context(), &document)
            .await
            .map_err(|error| {
                trace_failure::<D>("encode", &error);
                error
            })?;
        // Writer 在真实 I/O 失败位置记录 write/flush tracing。
        writer.write(context.clone()).await?;
        Ok(context)
    }
}

impl<P, D> Pipeline<P, D>
where
    P: Protocol,
    D: Direction,
{
    pub fn new(reader: Box<dyn ReadPipeline<P, D>>, writer: Box<dyn WritePipeline<P, D>>) -> Self {
        Self {
            reader,
            writer,
            protocol: PhantomData,
            direction: PhantomData,
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

fn hex(data: &[u8]) -> String {
    data.iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Stage 是 tracing 的结构化字段，不进入 Error 类型。
fn trace_failure<D: Direction>(stage: &'static str, error: &Error) {
    tracing::error!(
        target: "intercept_proxy::exchange::ui",
        event = "failed",
        direction = ?D::KIND,
        stage = stage,
        error = %error.message
    );
}
