# 安全、TLS 与持久化

本文说明 SQLite/Workspace/revision、证书与秘密材料、应用备份导入导出，以及 HTTP/Socket
TLS/mTLS 的双连接边界。运行时报文与 ExchangeObservation 不属于持久化配置。

## 1. 持久化所有权

```text
UI 用户意图
  -> Application 用例和领域校验
  -> Repository Port
  -> Infrastructure SQLite / 文件 / Keychain 或 DPAPI
```

前端不直接读写数据库。`domain` 定义实体与不变量，`application` 编排原子用例，`infrastructure`
实现 SQLite、文件、证书保护和导入导出。Tauri command 只完成 DTO/错误适配。

## 2. SQLite 当前保存什么

当前 schema version 为 19，主要表包括：

- `settings`：全局设置 JSON 和 revision；
- `workspaces`、`workspace_state`：完整 Workspace JSON、revision 与当前选择；
- `certificate_material`、`certificate_state`：受保护的证书材料和集合 revision；
- `protected_secrets`：provider/key 对应的受保护 blob；
- `protocol_packages`、`protocol_package_files`：精确包版本、校验状态和不可变源文件；
- `external_protocol_packages`：外部包注册指纹、启用状态、最近连接和稳定错误；
- `android_runtime_owner`：跨进程恢复所需的设备网络所有权事实；
- `application_feature_state`：一次性初始化状态。

HTTP capture、Socket ExchangeObservation 和运行时报文不创建数据库表；它们保存在有界内存。普通
运行日志使用独立 JSONL 文件，不写入 SQLite。

项目仍处于开发期：数据库版本不匹配时 Host 会清空旧预发布数据库并按当前 schema 重建，而不是
执行向后兼容迁移。不要把这一行为描述成已具备生产迁移能力。

## 3. 事务与并发

`SqliteStore` 持有一个 `rusqlite::Connection`，使用互斥锁明确单事务所有权。跨表替换使用
`TransactionBehavior::Immediate`；任何一步失败都回滚整个事务。

### 3.1 revision 乐观锁

Settings、Workspace、规则和证书集合均使用 revision/CAS：

1. 查询返回当前 revision；
2. 编辑命令必须带 `expected_revision`；
3. SQLite 在同一事务比较当前值；
4. 匹配才写入并递增；
5. 不匹配返回稳定 revision conflict，禁止后写覆盖先写。

运行时另有 `runtime_epoch`，用于区分一次 Listener 启动。revision 是配置并发身份，epoch 是运行实例
身份，二者不可混用。

### 3.2 Workspace 是聚合根

Workspace 保存 Listener、HTTP 基础规则、协议 Document 规则、Android Profile 和证书引用等用户
配置。协议包文件本体、证书私钥本体和运行任务不嵌入 Workspace JSON。

Listener 启动前，Application/Infrastructure 从 Workspace 和注册表构造不可变 runtime snapshot。
网络任务只持有该快照，不在报文处理中反复查询 SQLite。配置更新只有通过明确 replace/restart 路径
才影响运行时。

## 4. 秘密和证书材料

Workspace 只保存类型化 `CertificateReference`，引用用途包括：

- 本机 MITM Root CA；
- 下游 Server Identity；
- 下游 Client Trust；
- 上游 Server Trust；
- 上游 Client Identity。

SQLite 的证书/秘密列保存 protected blob。macOS 通过 Keychain 中的主密钥保护 envelope；Windows
使用 current-user DPAPI。明文私钥、P12 密码和密钥只在导入、解析、TLS plan 构造等短生命周期
内存边界出现，不进入普通 ViewModel 或 Debug 输出。

证书公开查询只返回 subject、issuer、指纹、有效期、用途和就绪状态等元数据。`MitmRootCa`
是本机安装身份，不作为可移植证书材料导出。

## 5. TLS 是两条独立连接

代理不是一条 TLS 隧道的“中间变量”，而是最多同时持有两条独立安全连接：

```text
App -- downstream TLS/mTLS --> Proxy -- upstream TLS/mTLS --> Server
```

### 5.1 downstream：Proxy 作为 Server

Proxy 需要 Server Identity 向 App 出示证书。mTLS 可配置：

- Disabled：不请求客户端证书；
- Optional：按指定 Client Trust 验证，允许未提供；
- Required：必须提供且通过 Client Trust 验证。

App 证书指纹属于 downstream peer 事实，可供 TLS 握手规则和诊断使用。

### 5.2 upstream：Proxy 作为 Client

Proxy 独立决定：

- 是否校验服务器主机名；
- 使用系统/默认信任还是明确 Server Trust；
- Socket 是否使用显式 `tls_server_name`；
- 是否携带 Client Identity 完成上游 mTLS。

上游认证结果不能沿用 App 到 Proxy 的 TLS session。真实 server hostname、CA、客户端身份和业务签名
输入必须保持各自语义。

### 5.3 Socket 安全拓扑

Socket Relay 用 tagged union 表达四种可执行拓扑：

| 配置 | App -> Proxy | Proxy -> Server |
| --- | --- | --- |
| `Transparent` | TCP | TCP |
| `TcpToTls` | TCP | TLS/mTLS |
| `TlsToTcp` | TLS/mTLS | TCP |
| `TlsToTls` | TLS/mTLS | TLS/mTLS |

LocalResponder 没有上游连接，使用独立的 `SocketDownstreamSecurity`，只能是 TCP 或下游 TLS；类型层面
不能表达无效的上游地址/信任/客户端身份。

HTTP 固定 Server 模式同样把 downstream TLS 与 `fixed_server.upstream_tls` 分开。MITM 动态证书签发
只解决 App 信任 Proxy，不能代替 Proxy 对真实上游的 hostname/CA 校验。

## 6. 配置快照中的 TLS

Listener 启动时，plan builder 校验证书引用用途并加载 DER/PKCS#8 到运行快照：

- downstream 配置得到 Server Identity、Client Trust 和 required 标志；
- upstream 配置得到 Server Trust、可选 Client Identity、hostname policy 和 server name；
- Direct/Transparent 或 LocalResponder 不解析与实际拓扑无关的证书；
- 快照 Debug 只输出数量/状态，不输出证书或私钥字节。

证书引用不存在、用途错误、解析失败、hostname 不匹配或 mTLS 身份无效时 fail-closed，Listener 不应
带着半配置 TLS 继续运行。

## 7. 应用数据导出

应用备份为 ZIP v1，`application.json` 使用严格 closed wire，二进制载荷放在独立安全相对路径：

```text
application.json
protocol-packages/<id>/<version>/...
portable-materials/...
```

导出先构造不可变 `ApplicationBackupExportSnapshot`，再写用户选择的目标。文档包含：

- 当前选择的 Workspace；
- 全部 Workspace；
- 可移植 Settings；
- 可移植内置协议包的精确版本、启用状态和源文件；
- 除安装级 MITM Root 外的 Listener TLS 可移植材料。

路径必须规范、相对、无 `..`/反斜杠/盘符，身份和路径严格排序且不重复。证书材料记录 SHA-256；
Debug 只显示材料数量和 password 是否存在。

## 8. 应用数据导入

导入是 prepare/preview/commit 两阶段流程：

1. 有界读取 ZIP，拒绝路径穿越、重复/冲突路径、过多文件、过大文件、总量和压缩比异常；
2. 严格解析 `application.json`，拒绝未知字段、运行态字段和禁止的敏感配置字段；
3. 恢复并完整编译所有协议包；
4. 校验证书材料哈希、引用用途、证书链/私钥和密码；
5. 构造候选配置与当前 baseline；
6. 保留有时限的 import token，UI 只展示替换范围；
7. commit 时再次比较 Workspace revision、Settings revision、包 generation/启用状态和证书 generation；
8. 一致时在事务中整体替换协议包注册表、Workspace、选择状态和 Settings；不一致则拒绝提交。

因此，本机已经存在相同软件包不是简单“插入重复键”错误：完整应用导入以备份注册表为权威，事务内
先删除旧注册表，再写入备份版本；任一步失败都会恢复原状态。精确身份相同但内容不同的处理也必须
经过备份候选的完整编译、baseline 比较和原子替换，不能绕过校验直接覆盖文件。

导入成功后需要重启，使所有 Listener、协议包执行器、证书和 runtime snapshot 从新配置重新构造。

## 9. 重置与恢复

“重置应用数据”会在事务中删除受保护秘密、证书、Android runtime owner、协议包和 Workspace，
递增证书状态，并写入唯一默认 Workspace/Settings；可选内置包在同一事务恢复。

Android runtime owner 单独持久化，是因为 ADB reverse 或设备断线可能跨进程遗留资源。它只保存
恢复/清理所需的端口和状态事实，不保存运行报文。

## 10. 验证逻辑

持久化与安全改动至少覆盖：

- stale revision 和并发写冲突；
- 多表事务中途失败后完整回滚；
- 数据库损坏、非法负 revision 和 schema version 重建；
- 相同/冲突协议包身份的导入替换；
- ZIP 路径、大小、压缩比、重复项和 Base64 规范；
- import token 过期、baseline 漂移和 commit 原子性；
- 证书引用用途、材料哈希、密码、私钥与 Debug/DTO 不泄漏；
- TCP/TLS、TLS/TCP、TLS/TLS 和 LocalResponder TLS；
- hostname/CA 校验、可选/必需下游客户端证书、上游客户端身份；
- TLS 失败不产生明文 fallback，观测记录明确失败阶段。
