# ADR-010：进程内 WebAssembly Component 协议包

- Status: Accepted
- 日期：2026-09-01
- Supersedes: [ADR-009](ADR-009-nested-document-javascript-package-runtime.md) 的本地 JavaScript ZIP 与 Boa Sidecar 决策

## Context

本地协议包原先作为 JavaScript ZIP 由独立 Boa Sidecar 执行，再通过应用自身的 `/packages`
WebSocket 回连。该路径增加了跨平台二进制打包、子进程生命周期、JSON-RPC 和 Base64 边界，也让本地
错误经过传输包装后损失直接的 Wasmtime/WIT 上下文。远程进程仍需要源码级快速调试入口，但本地已安装
包不需要网络传输。

## Decision

1. 本地协议包是一个 `.wasm` WebAssembly Component。唯一 `intercept-proxy:manifest` 顶层 custom
   section 保存 API 1 Manifest；导入边界验证 Component、Manifest 和 Manifest 选择的 WIT world。
2. Tauri/Rust 主进程通过 Wasmtime 实例化本地包，不启动 Sidecar，也不把本地调用发送到
   `/packages`。本地实例失败时 fail-closed，不自动回退到远程包或其他运行时。
3. Proxy Pipeline 只依赖传输无关的 `ProtocolPackageRuntime`：HTTP 输入输出是 UTF-8 字符串，Socket
   输入输出是原始字节，Document 是领域对象，Display 是字符串。远程 WebSocket 适配器独占
   JSON-RPC DTO 与 Base64 转换。
4. Component 通过版本化 WIT 导出固定的 Frame、Decode、Encode 和 Display Hook。HTTP world 使用
   `string`；Socket world 使用 `list<u8>`；Document 在 WIT 边界使用规范 JSON 字符串。
5. Host 向 Component 提供 WASI 文件系统、环境、网络能力和 `ws`/`wss` WebSocket 接口。产品不增加
   沙箱、fuel、内存、包体积、Hook 超时、Busy、重试或 replay 限制；宿主和操作系统自身的资源失败按
   真实错误传播。
6. 远程第三方软件包继续通过 `/packages` WebSocket 与 API 1 JSON-RPC 接入，用于 Python、JavaScript
   等源码语言快速调试。它是显式的远程来源，不是本地 Wasm 的执行步骤或失败回退。
7. UI 只展示协议包导入、导出、版本、能力和 Schema，不展示 Wasm 运行时选择或能力标签。

## Why

- 本地 Hook 是同进程、强类型调用，消除自连接、子进程和本地 Base64 成本。
- 传输编码只存在于远程适配器，内部接口使用领域原生类型，更难误用。
- 单文件 Component 让 Rust、C/C++、Go、Python 等可生成 Component 的语言共享同一导入合同。
- 远程调试路径继续保留完整源码诊断，不与交付格式绑定。

## Consequences

- 桌面包不再包含 `intercept-proxy-package-sidecar`，macOS Universal 与 Windows 发布无需额外 staging。
- 每个精确本地版本拥有一个 Wasmtime Store；Hook 调用按 Store 所有权串行化。
- Host WebSocket 是 Guest 可选调用的外部能力；普通协议 Hook 不经过 WebSocket。
- 完整 Proxy、真实链路与安装包验收在变更合并回主工作区后执行。

## Alternatives

- Rejected：本地 Component 仍通过 `/packages` 自连接；保留了不必要的网络、JSON-RPC 和 Base64 边界。
- Rejected：继续 Boa Sidecar；Windows 子进程交付和跨平台 staging 仍是产品依赖。
- Rejected：本地失败后自动调用远程同身份包；会形成隐式双路径并掩盖本地错误。
