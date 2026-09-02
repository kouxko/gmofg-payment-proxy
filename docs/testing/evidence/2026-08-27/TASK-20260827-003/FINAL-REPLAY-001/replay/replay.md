# 复测入口

从仓库根目录执行：

```bash
pnpm check
cargo test --manifest-path src-tauri/Cargo.toml --workspace --quiet
```

隔离 App 控制面复测：

1. 使用独立 identifier 构建 macOS App。
2. 启动 bundle 内二进制并等待 `17653` 监听。
3. 发送带当前 MCP protocol metadata 的 `resources/list` 和 `resources/read` 请求。
4. 读取 `intercept-proxy://docs/validation-playbook/1.0`。
5. 使用无私有材料的完整候选调用 create、apply、status，等待 `committed`。
6. 关闭 App，确认端口释放；重启并调用 capabilities；再次关闭并确认端口释放。

Android 真机和授权交易仅在资源满足时执行；资源不足必须保留 `NOT_RUN`。
