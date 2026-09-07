# INTEGRATED-VERIFICATION-001：已实现批次集成验证

任务 `TASK-20260905-001`。已实现 #1/#2/#4/#5/#6/#7/#8/#12；#3 等待恢复方式确认、#9 草稿 DTO 设计未就绪，均未改实现。当前检查与独立审查通过；不表示整个任务完成。

- Rust 四库：Android Engine 48、Application 418、Infrastructure 469、Runtime 185，共 1120/1120。日志 `rust-libraries.log`。
- 前端第一轮全量 573/573；最终冲突解决补充后受影响 216/216、typecheck/lint/boundary PASS，详见 [前端用例](../frontend-contracts/README.md)。573 不是最终补充后的全量计数。
- 最终 Next production build PASS，见 `frontend-build-final.log`；Rust 全量格式、架构扫描、diff 检查 PASS。
- Android JVM 32/32、四 ABI APK 构建/签名/对齐验证 PASS，见 [Android 用例](../android-contracts/README.md)。设备未连接，真机验证 NOT_RUN。
- 源码尺寸门禁仍有原有 7 项失败，本次未新增。没有将该门禁记为 PASS。

复测前使用仓库既有 Deno/Rust/Android 工具链，命令从仓库根目录执行；完整命令、HEAD、当前源文件快照及 SHA 位于 metadata.json。resources 保存集成验证涉及的修改源码和新增测试。测试输入、预期/实际、失败复现和专用资源分别在 [Android](../android-contracts/README.md)、[后端](../backend-contracts/README.md)、[前端](../frontend-contracts/README.md)、[路由](../strict-routing/README.md)、[活动文档](../current-documents/README.md) 归档。

```sh
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application -p intercept-proxy-infrastructure -p intercept-proxy-runtime -p intercept-proxy-android-engine --lib
```

```sh
deno task build
```

```sh
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
```

```sh
deno task scan:architecture
```

```sh
git diff --check
```

本用例无独立网络报文输入，真实 TCP/Header/SOCKS5 报文属于上述子用例；外部交易、TLS/mTLS、设备授权和 UI 真机截图均未作为本次已验证结果。没有执行 CI、推送或发布。

独立架构审查：已实现范围 `APPROVE`，新P0/P1/in-scope P2为0。详见[审查报告](independent-review.md)。整任务仍需#3/#9及适用真机验证，不能把此批次结论当作整任务完成。
