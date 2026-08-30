//! HTTP Exchange 的方向能力装配合同。
//!
//! Proxy 核心只认识独立的 Decode、Display、Rules、Encode 能力。Rhai、外部进程和 Rust
//! 内建实现都通过同一个 factory 注入，禁止把已经完成整条协议处理的旧 processor 包进
//! Decode 再用空 adapter 冒充其余阶段。

use std::{collections::BTreeMap, fmt::Debug, marker::PhantomData};

use async_trait::async_trait;
use intercept_proxy_exchange::{
    Decode, Direction, Display, Document, DocumentValue, Downstream, Encode, Error, Http,
    HttpContext, JsonPointer, Rules, Upstream,
};
use uuid::Uuid;

use crate::transport::ConnectionContext;

const HEADER_FIELD: &str = "header";
const BODY_FIELD: &str = "body";

/// 一条连接的稳定身份；能力实现可用它创建连接级脚本/RPC runtime。
#[derive(Clone, Debug)]
pub struct HttpConnectionIdentity {
    pub runtime_epoch: Uuid,
    pub connection_id: Uuid,
    pub peer: String,
}

impl From<&ConnectionContext> for HttpConnectionIdentity {
    fn from(context: &ConnectionContext) -> Self {
        Self {
            runtime_epoch: context.runtime_epoch,
            connection_id: context.connection_id,
            peer: context.peer_addr.to_string(),
        }
    }
}

/// Factory 冻结的 Workspace/Listener 归属；payload 不进入该元数据。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpObservationMetadata {
    pub workspace_id: String,
    pub listener_id: String,
}

/// 每连接、每方向独占的四项 HTTP 能力。
pub struct HttpDirectionCapabilities<D: Direction> {
    pub decode: Box<dyn Decode<Http, D>>,
    pub display: Box<dyn Display>,
    pub rules: Box<dyn Rules>,
    pub encode: Box<dyn Encode<Http, D>>,
}

impl<D: Direction> Debug for HttpDirectionCapabilities<D> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpDirectionCapabilities")
            .field("direction", &D::KIND)
            .finish_non_exhaustive()
    }
}

impl<D: Direction> HttpDirectionCapabilities<D> {
    pub fn new(
        decode: Box<dyn Decode<Http, D>>,
        display: Box<dyn Display>,
        rules: Box<dyn Rules>,
        encode: Box<dyn Encode<Http, D>>,
    ) -> Self {
        Self {
            decode,
            display,
            rules,
            encode,
        }
    }
}

/// 为一条 HTTP connection 创建 upstream/downstream 两组真实能力。
pub trait HttpProtocolCapabilityFactory: Debug + Send + Sync {
    fn observation_metadata(&self) -> HttpObservationMetadata;

    fn create_upstream(
        &self,
        connection: HttpConnectionIdentity,
    ) -> Result<HttpDirectionCapabilities<Upstream>, Error>;

    fn create_downstream(
        &self,
        connection: HttpConnectionIdentity,
    ) -> Result<HttpDirectionCapabilities<Downstream>, Error>;
}

/// 顺序执行多条 Rules；空链表示该方向没有配置规则，不是兼容性 fallback。
pub struct RulesChain {
    rules: Vec<Box<dyn Rules>>,
}

impl Debug for RulesChain {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RulesChain")
            .field("rule_count", &self.rules.len())
            .finish()
    }
}

impl RulesChain {
    #[must_use]
    pub fn new(rules: Vec<Box<dyn Rules>>) -> Self {
        Self { rules }
    }
}

#[async_trait]
impl Rules for RulesChain {
    async fn apply(&mut self, mut document: Document) -> Result<Document, Error> {
        for rules in &mut self.rules {
            document = rules.apply(document).await?;
        }
        Ok(document)
    }
}

/// 未绑定协议包时使用的明确 Rust 文本能力。
#[derive(Debug)]
pub struct PlainHttpCapabilityFactory {
    metadata: HttpObservationMetadata,
}

impl PlainHttpCapabilityFactory {
    #[must_use]
    pub fn new(workspace_id: impl Into<String>, listener_id: impl Into<String>) -> Self {
        Self {
            metadata: HttpObservationMetadata {
                workspace_id: workspace_id.into(),
                listener_id: listener_id.into(),
            },
        }
    }

    fn build<D: Direction>() -> HttpDirectionCapabilities<D> {
        HttpDirectionCapabilities::new(
            Box::new(TextDecode::<D>(PhantomData)),
            Box::new(TextDisplay),
            Box::new(RulesChain::new(Vec::new())),
            Box::new(TextEncode::<D>(PhantomData)),
        )
    }
}

impl HttpProtocolCapabilityFactory for PlainHttpCapabilityFactory {
    fn observation_metadata(&self) -> HttpObservationMetadata {
        self.metadata.clone()
    }

    fn create_upstream(
        &self,
        _connection: HttpConnectionIdentity,
    ) -> Result<HttpDirectionCapabilities<Upstream>, Error> {
        Ok(Self::build())
    }

    fn create_downstream(
        &self,
        _connection: HttpConnectionIdentity,
    ) -> Result<HttpDirectionCapabilities<Downstream>, Error> {
        Ok(Self::build())
    }
}

struct TextDecode<D: Direction>(PhantomData<fn() -> D>);

#[async_trait]
impl<D: Direction> Decode<Http, D> for TextDecode<D> {
    async fn decode(&mut self, context: &HttpContext) -> Result<Document, Error> {
        let mut document = Document::new(DocumentValue::Object(BTreeMap::default()));
        document
            .set(
                &JsonPointer::property(HEADER_FIELD),
                DocumentValue::String(context.header.clone()),
            )
            .map_err(|error| domain_error(&error))?;
        document
            .set(
                &JsonPointer::property(BODY_FIELD),
                DocumentValue::String(context.body.clone()),
            )
            .map_err(|error| domain_error(&error))?;
        Ok(document)
    }
}

struct TextDisplay;

#[async_trait]
impl Display for TextDisplay {
    async fn display(&mut self, document: &Document) -> Result<String, Error> {
        text(document, BODY_FIELD).cloned()
    }
}

struct TextEncode<D: Direction>(PhantomData<fn() -> D>);

#[async_trait]
impl<D: Direction> Encode<Http, D> for TextEncode<D> {
    async fn encode(
        &mut self,
        _original: &HttpContext,
        document: &Document,
    ) -> Result<HttpContext, Error> {
        Ok(HttpContext {
            header: text(document, HEADER_FIELD)?.clone(),
            body: text(document, BODY_FIELD)?.clone(),
            body_is_utf8: true,
            wire_body: text(document, BODY_FIELD)?.as_bytes().to_vec(),
        })
    }
}

fn text<'a>(document: &'a Document, field: &str) -> Result<&'a String, Error> {
    match document
        .resolve(&JsonPointer::property(field))
        .map_err(|error| domain_error(&error))?
    {
        DocumentValue::String(value) => Ok(value),
        _ => Err(Error::new(format!(
            "HTTP_DOCUMENT_INVALID\n{field} must be a String"
        ))),
    }
}

fn domain_error(error: &intercept_proxy_exchange::DomainError) -> Error {
    Error::new(format!("{}\n{}", error.code, error.message))
}

#[cfg(test)]
mod tests {
    use intercept_proxy_exchange::{Decode, Encode, HttpContext, Rules};

    use super::{PlainHttpCapabilityFactory, TextDecode, TextEncode, Upstream};
    use std::marker::PhantomData;

    #[tokio::test]
    async fn plain_capabilities_round_trip_header_and_body() {
        let context = HttpContext {
            header: "POST /sale HTTP/1.1\r\n\r\n".into(),
            body: "0200".into(),
            body_is_utf8: true,
            wire_body: b"0200".to_vec(),
        };
        let mut decode = TextDecode::<Upstream>(PhantomData);
        let document = decode.decode(&context).await.unwrap();
        let mut rules = super::RulesChain::new(Vec::new());
        let document = rules.apply(document).await.unwrap();
        let mut encode = TextEncode::<Upstream>(PhantomData);

        assert_eq!(encode.encode(&context, &document).await.unwrap(), context);
        let _ = PlainHttpCapabilityFactory::new("workspace", "listener");
    }
}
