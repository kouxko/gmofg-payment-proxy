use tokio::io::{AsyncRead, AsyncWrite};

pub trait IoStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> IoStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub type BoxIo = Box<dyn IoStream>;
