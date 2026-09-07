# Wasm 合并后本地整体验证

- 被测提交：`8cac832`
- 被测 App：`src-tauri/target/release/bundle/macos/Intercept Proxy.app`
- 构建时间：2026-09-01 21:48:47 +08:00
- 测试时间：2026-09-01 21:49–21:56 +08:00
- 结果：部分通过；核心 Wasm/Proxy 业务链通过，协议包详情 UI 存在展示问题。

## 已验证

1. 发布门、Next/TypeScript、Tauri arm64 release App 构建通过。
2. Plain HTTP 真实链路通过：Method、Header、request target wildcard、Body RFC6901 路径、全部命中、全部未命中共 6 个有效请求；非法 JSON fail-closed。
3. 内置 ISO8583 Wasm Socket 真实链路通过：0200 改写为 0100；0400 字节保持；非法 Frame fail-closed 且未到达上游。
4. AU EFTEX 1.1.0 Wasm 导入成功，Host API 1、Socket、双向 97 个 Schema 节点、Frame/Decode/Encode/Display 能力均通过预览校验。
5. App 重启后 AU EFTEX 与内置 ISO8583 均恢复为 `enabled=true`、`online=true`、`valid`。

## 已记录问题

- 内置 ISO8583 与新导入 AU EFTEX 的详情页均显示“协议包详情读取失败 / 数据不完整”；MCP `protocol_package_detail` 返回完整能力、Schema、managed runtime 与版本信息，因此问题限定在详情 UI 展示/映射。
- 启动时旧本地 Workspace v8 含已删除的 `actions` 字段，当前合同 fail-closed；旧数据库已移动到可恢复目录 `~/Library/Application Support/com.interceptproxy.desktop/pre-wasm-smoke-20260901-2200/`，随后用 Schema100 空库完成测试。

## 按用户要求未测试

- Wasm Host 文件系统能力。
- WASI 出站 HTTP。
- Host WebSocket 等同类外部能力。

## 清理与留存

- 8080、8081、18083、18084 已释放。
- App 保持运行，PID 22901；8765 与 17653 保持监听，供人工检查。
- 未修改产品源码。
