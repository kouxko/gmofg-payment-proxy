# 复测入口

从仓库根目录执行：

```bash
CI=true pnpm tauri build --bundles app \
  --config '{"identifier":"com.interceptproxy.desktop.g032test","bundle":{"macOS":{"signingIdentity":"-","hardenedRuntime":false}}}'
```

启动 bundle 内二进制后，按 `src-tauri/src/mcp/tests.rs` 的 production HTTP framing 调用：

1. `environment_candidate_create`，arguments.candidate 使用
   `../resources/full-resource-candidate.json`。
2. 从 create structuredContent 提取 `candidate_id` 和 `confirmation_token`。
3. 调用 `environment_candidate_apply`，再轮询 `environment_candidate_status` 到终态。
4. 退出 App，检查隔离数据库中 Workspace 的 `_persistence_version` 与资源数组长度。
5. 再次启动 App，确认 `17653` 的 production IPv4/IPv6 Listener 恢复且 MCP 请求成功。

对应自动化回归：

```bash
cargo test --manifest-path src-tauri/Cargo.toml \
  production_ports_commit_full_resource_candidate_with_builtin_exact_package \
  --all-features -- --nocapture

cargo test --manifest-path src-tauri/Cargo.toml \
  production_ports_commit_minimal_new_workspace_with_builtin_package_inventory \
  --all-features -- --nocapture
```

复测结束后停止 App，确认端口 `17653` 释放，并清理隔离 identifier 的数据目录。不要删除或改写正式
`com.interceptproxy.desktop` 数据目录。
