# 复测步骤

1. 构建并 seal 当前 arm64 App：

   ```bash
   pnpm exec tauri build --target aarch64-apple-darwin --bundles app
   node scripts/sign-macos-app.mjs src-tauri/target/aarch64-apple-darwin/release/bundle/macos
   codesign --verify --deep --strict --verbose=2 'src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Intercept Proxy.app'
   ```

2. 用独立 `HOME` 启动 App，创建 Plain HTTP Listener `127.0.0.1:8080`，上游指向 `127.0.0.1:18083`。
3. 在规则页创建上行规则：条件 `/customer/age`、number、equals、18；动作 `RecordMatch`；保存并启动 Listener。
4. 向 `http://127.0.0.1:18083/body-match?case=age18` 和 `age17` 发送 `inputs/match.json`、`inputs/miss.json`，HTTP proxy 为 `127.0.0.1:8080`。
5. 发送 `inputs/invalid.txt`，确认运行日志为 `JSON_INVALID` 且本地 Server 未收到请求。
6. 在规则编辑器选择 `Jitter`，确认动作类型和创建按钮同一行，参数 textarea 独占下一行；确认 Document 区显示“匹配值”“动作值”，不存在“JSON 值”。
7. 退出 App 和 Server，检查 8080、8765、17653、18083 无监听。
