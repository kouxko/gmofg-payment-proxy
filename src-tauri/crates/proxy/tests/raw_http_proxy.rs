//! `TEST-PROXY` 与 `TEST-CONCURRENCY` 的原始 TCP/HTTP 集成测试。
//!
//! 这里从真实本机 socket 驱动 Listener、连接服务和 pipeline，验证并发、容量、取消与
//! 原始 HTTP 行为；上游和证书仍是测试替身，不能把通过结果直接当成 GMO-FG 真机证据。

include!("raw_http_proxy/support.rs");
include!("raw_http_proxy/wire_fidelity.rs");
include!("raw_http_proxy/lifecycle.rs");
include!("raw_http_proxy/capacity_and_faults.rs");
