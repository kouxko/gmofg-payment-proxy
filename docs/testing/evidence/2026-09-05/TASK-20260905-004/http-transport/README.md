# HTTP 公共传输与下游写出边界

任务：TASK-20260905-004；执行跨 2026-09-05 / 2026-09-06，Asia/Shanghai。

目的：固定上游与 Forward 共用 HTTP/1 egress、体积限制和取消/超时；下游 Body 被消费与真正 flush 分离。保留原始首行、非 UTF-8 头/Body、1xx、chunked、故障注入和现有流式接口。

## 输入、预期与实际

- `write_commit.rs` 使用内联的固定响应字节和 duplex/PendingFlush I/O。Body EOF 后 receipt 必须仍 pending；完整 write 后仍 pending；flush 成功才收到 Ok；flush 阻塞超时返回失败。
- RED：`outputs/red-response-commit.log` 显示旧实现把 Body EOF 当作完成，断言失败。
- 基线：`outputs/baseline-http.log` 17/17；`outputs/baseline-forward-all.log` 17/17。`baseline-forward.log` 的过滤器未匹配测试，不能作为通过证据。
- 集成期间保存的 `downstream-check*.log` 和 `shared-egress-runtime*.log` 包含编译暂态与失败；这些不是最终通过证据。`green-downstream-http.log` 实际是旧测试接口编译失败，文件名不作为结果。
- 最终结果与完整日志见当日 [集成验证](../../../2026-09-06/TASK-20260905-004/integrated-verification/README.md)。

## 复测

从仓库根目录执行：

```sh
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime --lib write_commit -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime --test raw_http_proxy -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-runtime --test upstream_http -- --nocapture
```

资源是当次实际测试源码（内联输入、预期与 I/O fixture）及 wire 保真回归；保留源路径结构。无需启动应用或外部服务。真实设备、远程服务器、UI、CI：N/A，本例没有这些边界。Local 默认响应 Q1 尚待确认，此用例不声称其已修复。
