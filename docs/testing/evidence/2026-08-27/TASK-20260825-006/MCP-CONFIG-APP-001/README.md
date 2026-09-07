# MCP-CONFIG-APP-001

## 目的

在隔离产品标识的真实 macOS `.app` 上验证环境候选的完整资源预览、确认应用、`SQLite` 持久化
封装和 App 重启恢复。该用例不发送业务报文，也不证明交易、Frame、Decode/Encode 或远端服务成功。

## 环境与被测对象

- 日期：2026-08-27（Asia/Shanghai）
- 平台：macOS arm64
- App：`src-tauri/target/release/bundle/macos/Intercept Proxy.app`
- 隔离 Tauri identifier：`com.interceptproxy.desktop.g032test`
- MCP：明文 Streamable HTTP，production IPv4/IPv6 wildcard Listener，端口 `17653`
- 测试数据目录：独立 Application Support 目录；没有读取或修改正式 identifier 的数据目录。
- 源码说明：使用本证据 `inputs/` 中的任务文件清单和关键源码摘录。

## 输入

- [完整资源候选](resources/full-resource-candidate.json)
- 新 Workspace：`G032 Packaged Full Resources`
- 资源：2 个停止状态 Listener、14 条 HTTP Rule、1 条 Protocol Document Rule、1 个 Android
  Profile、内置 `iso8583-ascii-standard@1.0.0` 精确引用。
- 私有材料：无；证书与秘密数组为空，避免真实 Keychain 命名空间参与本次打包冒烟。

## 执行步骤

1. 使用隔离 identifier 构建并 ad-hoc 签名 `.app`。
2. 启动 bundle 内的 production 二进制，确认 IPv4/IPv6 wildcard Listener 均监听 `17653`。
3. 调用 `environment_candidate_create`，逐字段读取 preview 与 7 层 validation 结果。
4. 使用同一响应中的一次性 confirmation token 调用 `environment_candidate_apply`。
5. 轮询 status，直到 `committed`。
6. 退出 App，直接读取隔离 `SQLite` 记录的持久化版本和资源数量。
7. 再次启动同一个 `.app`，确认不发生 `PERSISTENCE_CORRUPT`，MCP Listener 可重新服务。

## 预期与实际

- 预期：7 层中适用层全部 `passed`，TLS 无目标时 `not_applicable`；实际：PASS。
- 预期：apply ACK 为 `apply_queued`，终态为 `committed`；实际：PASS。
- 预期：Workspace 使用当前持久化 envelope；实际：`_persistence_version=6`。
- 预期：持久化资源为 2/14/1/1；实际：2 Listener、14 HTTP Rule、1 Protocol Rule、1 Android Profile。
- 预期：退出后可重启并恢复；实际：PASS，IPv4/IPv6 Listener 再次绑定并接受 MCP 请求。
- 预期：正式数据目录不变；实际：PASS，本用例只使用隔离 identifier。

结构化结果见 [result.json](outputs/result.json)。原始/脱敏交互、HTTP headers、SQLite 查询、两次
Listener 状态、运行日志和构建输出均在 [outputs](outputs/)；实际请求在 [inputs](inputs/)，其中
一次性 confirmation token 已消费并替换为明确占位符。任务文件清单和关键源码摘录也在 `inputs/`。
复测入口见 [replay](replay/README.md)。

## N/A

- 业务 TCP/HTTP 交换、Frame、Decode、Rules、Encode、MAC、密码学和远端业务结果：N/A，本任务验证
  环境配置控制面，不发送业务 payload。
- 私钥、密码与真实证书导入：N/A，本次打包冒烟不使用私有材料；相邻材料解析、保护、清理和原子回滚
  由源码回归覆盖。
- 远程 Windows 验证：N/A，本用例只记录本地隔离 App 结果。
