# Managed Component 详情隔离回归

## 目的

验证本地托管 Wasm Component 的详情查询不会读取或展示只属于远端 `/packages` 连接的元数据，同时协议包前端在当前生成合同下完整通过。

## 派生关系

- 父任务：`TASK-20260901-001`
- 父用例：`wasm-integrated-runtime`
- 父证据：`../wasm-integrated-runtime/README.md`
- 变化：父用例执行时详情页结论为 `PARTIAL_PASS`；本用例在修复后补充 Rust 查询边界和前端完整回归。

## 被测对象

- `src-tauri/crates/application/src/facade/protocol_packages.rs`
- `src-tauri/crates/application/src/requirements_tests/protocol_package_lifecycle/queries.rs`
- `src/features/protocol-packages/`
- 当前共享工作区冻结源码，执行时间 `2026-09-01 23:11:55 +08:00` 至 `23:18:03 +08:00`。

## 步骤与结果

1. 执行精确 Rust 测试：

   ```bash
   cargo test --manifest-path src-tauri/Cargo.toml \
     -p intercept-proxy-application --lib \
     requirements_tests::protocol_package_lifecycle::queries::managed_component_detail_omits_remote_connection_metadata \
     -- --exact --nocapture
   ```

   结果：`1 passed / 0 failed`；托管详情 `external == None`，远端 detail port 调用次数为 `0`。

2. 执行协议包前端回归：

   ```bash
   pnpm exec vitest run src/features/protocol-packages
   ```

   结果：`7 files / 77 tests passed`，详情、列表、导入、来源和生命周期用例全部通过。

## 判定

- `VERIFIED`：托管 Component 详情不读取远端连接元数据。
- `VERIFIED`：协议包当前前端合同完整通过。
- UI 截图：`N/A`，本用例验证查询边界和现有组件测试，不执行新的人工点击。
- 网络、文件系统、报文：`N/A`，不属于本查询回归。
