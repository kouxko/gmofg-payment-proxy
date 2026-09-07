//! HTTP 写准备完成后的独立能力。

use async_trait::async_trait;
use intercept_proxy_exchange::Error;

use super::{HttpRequestMetadata, Message};

/// Final HTTP transformation after Encode and HTTP rule actions.
///
/// The native `Message` remains authoritative so raw header bytes are never
/// projected through `HttpContext` during Finalize.
#[async_trait]
pub trait Finalize: Send {
    async fn finalize(
        &mut self,
        request: &HttpRequestMetadata,
        message: &mut Message,
    ) -> Result<(), Error>;
}

#[derive(Debug, Default)]
pub struct NoopHttpFinalize;

#[async_trait]
impl Finalize for NoopHttpFinalize {
    async fn finalize(
        &mut self,
        _request: &HttpRequestMetadata,
        _message: &mut Message,
    ) -> Result<(), Error> {
        Ok(())
    }
}
