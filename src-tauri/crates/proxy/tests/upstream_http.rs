//! `PROXY-006..010` / `TEST-PROXY` 的原始上游 HTTP/1.1 集成测试。
//!
//! 临时 TCP Server 会记录 Proxy 实际写出的请求并返回手工响应，从而验证请求目标、
//! Header、Body 与连接关闭语义。该层不进行 TLS，也不解释 Payment 业务 JSON。

include!("upstream_http/support_and_fidelity.rs");
include!("upstream_http/limits.rs");
include!("upstream_http/connection_faults.rs");
include!("upstream_http/disconnects.rs");
