# Proxy 上游多 CA PEM Bundle 支持

## 任务信息

- 任务 ID：TASK-20260825-005
- 状态：已完成
- 任务日期：2026-08-25
- 创建时间：2026-08-25 20:44:12 +08:00
- 开始时间：2026-08-27 14:45:00 +08:00
- 最后更新时间：2026-08-27 15:27:02 +08:00
- 完成时间：2026-08-27 15:27:02 +08:00
- 创建路径：`docs/tasks/pending/2026-08-25/upstream-multi-ca-pem-bundle.md`
- 归档路径：`docs/tasks/completed/2026-08-27/upstream-multi-ca-pem-bundle.md`
- 关联提交：纳入最终整合交付
- 关键词：Socket、上游 TLS、多 CA、PEM Bundle、证书链、OpenSSL、MCP、兼容性
- 任务优先级：高
- 优先级理由：涉及 TLS 信任、安全校验、持久化恢复和运行时公共行为，失败会造成错误信任结果或连接失败。

## 历史和当前事实分级

### 当前已验证

- 当前上游信任导入路径调用单证书解析接口 `parse_upstream_ca`。
- Socket 和 Reverse TLS 配置的运行时信任材料类型已经是证书集合 `Vec<Vec<u8>>`。
- MCP 已将 `docs/mcp/certificate-concepts.md` 作为只读内嵌资源公开。
- 用户提供的 `sub.pem` 能由用户提供的 `DigiCertCA.pem` 验证，OpenSSL 输出为 `sub.pem: OK`。
- 两张证书分别是 `First Data Latvia Internal CA` 和 `First Data Baltics root CA`，有效期均覆盖当前日期。
- `195.160.171.102:63002` 当前可直接建立 TLS 1.3 连接；使用 Intermediate + Root Bundle 时
  OpenSSL 证书链验证为 `OK`。
- 目标后台当前仍只发送 `CN=test Axium` 叶子证书，并请求由 `C=CN, O=PAX, CN=PAX RCA R01`
  签发的客户端证书；本次无客户端身份探测仍完成 TLS 握手，但未发送业务报文。
- 只导入 Intermediate 时当前验证失败为 `unable to get issuer certificate`；对目标 IP 启用 IP 校验时
  当前验证失败为 `IP address mismatch`。
- `tangodev.nuvei.com:9081` 当前可以完成 TLS 1.3 握手，服务端证书为公开可信的 `CN=*.nuvei.com`；
  普通 HTTP `HEAD /` 和标准 HTTPS Proxy `CONNECT 195.160.171.102:63002` 均在 20 至 25 秒内无响应。

### 未知

- 实现完成时远端服务的在线状态、证书、TLS 策略和客户端证书要求是否发生变化。
- 真实业务报文能否完成交换；本任务只验证上游 Socket TLS 信任材料和握手分层。

## 背景

Proxy 需要连接以下上游 Socket TLS 服务：

```text
Host: 195.160.171.102
Port: 63002
Transport: TCP
Upstream security: TLS
```

上游证书链需要以下两张 CA：

```text
First Data Baltics root CA
└── First Data Latvia Internal CA
    └── test Axium Server Certificate
```

远端未发送 Intermediate CA 时，Proxy 只导入 Root CA 无法补齐链。用户要求在保留现有证书格式的
基础上，让一个规范化 PEM 文件能够同时承载 Intermediate CA 和 Root CA，并由 Proxy 完整加载。

2026-08-26 用户补充：外部环境新增代理节点，原文档中的代理地址改为
`https://tangodev.nuvei.com:9081`，配置路径也发生变化；本任务目标不是把该节点加入数据面，而是先
证明直连真实后台能够握手，再让 Intercept Proxy 绕过该节点直接转发到真实后台。

## 需求确认记录

以下内容由用户在当前会话明确确认：

- 本任务只描述和修改 Proxy，不列出或修改客户端应用侧要求。
- 目标数据面是 Socket 上游 TLS，目标地址为 `195.160.171.102:63002`。
- 不删除、不替换任何当前已支持的证书格式或历史证书记录。
- 新增支持一个规范化 PEM Bundle 文件包含多张 CA 证书。
- Proxy 不增加多文件选择和自动合并功能。
- 多个独立 PEM 文件由 OpenSSL 在 Proxy 外组合成一个 Bundle。
- MCP 只提供可复制的 OpenSSL 合并和验证指令，不执行 Shell、不写文件、不导入证书、不修改配置。
- 用户已经提供两张实际测试证书：`sub.pem` 和 `DigiCertCA.pem`。
- 测试资源完整归档，不设置隐私内容排除项。
- 2026-08-26 用户明确本轮只使用 Python 脚本验证握手并给出环境配置，不修改 Proxy 生产源码；
  本任务的多 CA 产品实现继续保持待实现。
- 2026-08-27 用户要求继续完成全部待办并在最终交付前重跑归档场景；本任务恢复进入产品实现。

## 需求就绪检查

- 问题、目标、范围、不在范围、输入输出、错误行为和真实目标均已明确：`PASS`。
- 归档 Intermediate、Root、目标地址与可复测步骤完整：`PASS`。
- 多成员任一失败必须整体拒绝，旧单证书格式保持兼容：`PASS`。
- 改变实现方向的未确认事项：`0`。
- 进入实现时间：`2026-08-27 14:45:00 +08:00`。

## 未确认事项

无阻塞实现的未确认需求。

以下行为沿用现有合同，不在本任务中自行扩展：

- 非证书 PEM Block、损坏证书、空文件和不适合作为上游信任材料时的错误语义；
- 重复证书的处理方式；提供的测试 Bundle 不包含重复证书；
- hostname/IP 验证和上游客户端身份策略；
- HTTP 上游 TLS 行为；只执行不回归检查，不增加 HTTP 产品能力；
- 数据库 Schema；除非当前存储结构无法保存完整 Bundle，否则不得新增迁移。

本次 Python 运行配置采用当前握手证据已经证明可行的显式合同：目标地址为 IP，服务端证书没有
IP SAN，因此 Python `SSLContext` 设置 `CERT_REQUIRED` 并显式 `check_hostname=false`。这只关闭名称
匹配，不关闭证书链验证；信任材料仍加载完整 Intermediate + Root Bundle。不得把该设置推广为其他
连接的默认值。

## 目标

- 现有证书格式和历史记录全部继续工作。
- 现有上游信任导入入口能够读取一个包含两张或更多证书的规范化 PEM Bundle。
- Bundle 中的全部证书被完整持久化，Proxy 重启后仍能恢复。
- Socket 上游 TLS 的 Trust Store 加载 Bundle 中的全部证书，而不是只加载第一张。
- MCP 证书说明提供经过验证的 OpenSSL 合并、查看、链验证和握手指令。
- 使用已归档的两张证书和目标后台完成可复测的分层验收。

## 实现范围

### 证书导入与规范化

- PEM 输入存在多个 `CERTIFICATE` Block 时解析全部证书。
- 将多证书输入重新编码为稳定、可再次解析的规范化 PEM Bundle。
- 单证书 PEM 和现有 DER/CRT/CER 路径保持原有行为。
- 解析失败继续使用现有强类型错误和 fail-closed 行为，不增加静默忽略或成功兜底。

### 持久化和恢复

- 托管的 `upstream_server_trust` 材料保存完整规范化 Bundle。
- 解析、导出、备份、恢复和可移植验证不得只保留第一张证书。
- 旧单证书记录不迁移、不重写，继续作为单成员信任集合加载。

### Socket TLS 运行时

- 将 Bundle 解析结果转换为现有运行时证书集合。
- Socket OpenSSL Trust Store 加载全部成员。
- 运行快照中的 `server_trust_count` 能反映实际加载数量。

### UI 和 MCP

- 继续使用现有单文件上游信任导入入口。
- 导入说明明确一个 PEM 文件可以包含多个 CA；不新增多文件选择器。
- MCP 内嵌证书概念资源提供本任务确认的 OpenSSL 指令。
- MCP 保持只读，不新增任意 Shell、文件写入或配置 mutation 能力。

## 不在范围

- 客户端应用侧配置、证书替换或 TLS 开关。
- Proxy 内多文件选择、拖动排序或自动合并。
- 生成或签发证书。
- 自动从远端下载 Intermediate CA。
- 关闭证书链、hostname/IP 或客户端身份验证。
- 新增协议回退、TLS 降级、重试或失败后透明转发。
- 修改 HTTP TLS 产品语义。
- 解决或伪造 PAX 客户端身份证书。
- 证明业务报文、MAC、加解密或交易结果成功。

## OpenSSL 指令合同

MCP 和文档提供以下组合命令：

```bash
{
  openssl x509 -in sub.pem -outform PEM
  openssl x509 -in DigiCertCA.pem -outform PEM
} > FirstData-trust-chain.pem
```

查看 Bundle 成员：

```bash
openssl crl2pkcs7 \
  -nocrl \
  -certfile FirstData-trust-chain.pem |
openssl pkcs7 -print_certs -noout
```

验证 Intermediate CA：

```bash
openssl verify -CAfile DigiCertCA.pem sub.pem
```

验证目标 TLS：

```bash
openssl s_client \
  -connect 195.160.171.102:63002 \
  -CAfile FirstData-trust-chain.pem \
  -showcerts \
  -verify_return_error
```

## 已提供和已归档测试资源

测试资源已经准备在：

```text
docs/testing/evidence/2026-08-25/TASK-20260825-005/TLS-CA-BUNDLE-001/
```

| 资源 | 仓库路径 | 用途 | 当前验证 |
| --- | --- | --- | --- |
| Intermediate CA | `resources/sub.pem` | 补齐上游缺失的 Intermediate | OpenSSL 可解析，Root 验证为 OK |
| Root CA | `resources/DigiCertCA.pem` | 上游证书链信任根 | OpenSSL 可解析且为自签名 Root |
| 目标配置 | `inputs/backend.json` | 固定 Host、Port、协议和预期证书数量 | 已准备 |
| 复测步骤 | `steps/replay.md` | 实现后从零组合 Bundle 并执行测试 | 已准备 |
| 资源验证输出 | `outputs/resource-validation.txt` | 保存 OpenSSL 版本、Subject、Issuer、有效期、指纹和链验证结果 | PASS |

证据入口：[TLS-CA-BUNDLE-001](../../../testing/evidence/2026-08-25/TASK-20260825-005/TLS-CA-BUNDLE-001/README.md)。

## 最小改动与最优设计

| 方案 | 修改范围 | 优点 | 风险或代价 | 结论 |
| --- | --- | --- | --- | --- |
| 最小正确改动 | 扩展上游信任材料解析/持久化，使其返回全部 DER；复用现有 Socket `Vec<Vec<u8>>` Trust Store；更新现有导入文案和 MCP 资源 | 不改领域模型和数据库 Schema，直接修复单证书截断根因 | 必须检查 metadata、portable、backup 等所有单证书调用点，避免遗漏 | 优先采用 |
| 新建全局证书链领域模型 | 新增跨层 Bundle 类型、DTO、Schema 和 UI 明细 | 类型表达更丰富 | 当前需求不需要，扩大公共接口和迁移面 | 不采用 |
| 只在 Socket connector 临时拆 PEM | 不改导入和存储，只在连接前重新解析 | diff 看似最小 | 持久化、metadata、导出恢复仍可能截断，形成第二条解析路径 | 拒绝 |

实施前先用回归测试确认现有存储材料能够保存完整 PEM 字节。若成立，采用最小正确改动，不新增
Schema，也不引入并行旧/新实现。

## 小任务列表

| ID | 小任务 | 依赖 | 可并行 | 负责人 | 状态 | 验收标准 | Commit | 小任务审查 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| T1 | 锁定单证书兼容行为和多证书失败基线 | 无 | 否 | 主 Agent | 已完成 | 单 PEM、DER 继续通过；多 PEM 用例先红 | 最终整合交付 | 已执行 |
| T2 | 实现 PEM Bundle 解析、规范化、持久化和恢复 | T1 | 否 | 主 Agent | 已完成 | 两张证书完整往返，旧记录无需迁移 | 最终整合交付 | 已执行 |
| T3 | 接入 Socket TLS Trust Store 和运行快照 | T2 | 否 | 主 Agent | 已完成 | Trust Store 与 snapshot count 均为 2 | 最终整合交付 | 已执行 |
| T4 | 更新导入说明、MCP OpenSSL 指令和相关文档 | T2 | 是，不与 T3 修改共享文件 | 主 Agent | 已完成 | 单文件 Bundle 说明准确，MCP 仍为只读 | 最终整合交付 | 已执行 |
| T5 | 使用归档资源执行回归和真实后台分层验收 | T3、T4 | 否 | 主 Agent | 已完成 | 证书链成功；其余失败准确归类 | 最终整合交付 | 已执行 |
| T6 | 文档同步、整体对抗审查、修复、复验和归档 | T5 | 否 | 主 Agent | 已完成 | 对抗审查无未关闭阻断，证据和索引完整 | 最终整合交付 | 已执行 |

## 测试计划

### TLS-CA-BUNDLE-001：资源准备与 OpenSSL 基线

- 状态：资源验证 PASS，Proxy 功能 NOT_RUN。
- 输入：已归档的 `sub.pem`、`DigiCertCA.pem` 和 `backend.json`。
- 预期：两张证书可解析，`openssl verify` 输出 `sub.pem: OK`，可生成包含两个成员的 Bundle。
- 证据：`docs/testing/evidence/2026-08-25/TASK-20260825-005/TLS-CA-BUNDLE-001/`。

### CERT-BUNDLE-UNIT-001：解析和规范化

- 输入：由归档资源生成的 `FirstData-trust-chain.pem`。
- 预期：解析结果恰好包含两张证书，Subject 和 DER 证书指纹与资源基线一致。
- 失败路径：第二个证书损坏时整体失败，不返回单证书成功结果。
- 证据：`docs/testing/evidence/<执行日期>/TASK-20260825-005/CERT-BUNDLE-UNIT-001/`。

### CERT-BUNDLE-PERSIST-001：持久化和重启恢复

- 输入：同一 Bundle，通过现有上游信任导入入口导入。
- 预期：保存、重新加载、导出/恢复后的证书数量和证书身份均为两张；旧单证书记录仍为一张。
- 证据：`docs/testing/evidence/<执行日期>/TASK-20260825-005/CERT-BUNDLE-PERSIST-001/`。

### SOCKET-TLS-BUNDLE-001：Socket 运行时

- 输入：Listener 上游 TLS 显式信任已导入 Bundle。
- 预期：运行计划和 OpenSSL Trust Store 加载两张证书，snapshot `server_trust_count=2`。
- 证据：`docs/testing/evidence/<执行日期>/TASK-20260825-005/SOCKET-TLS-BUNDLE-001/`。

### LEGACY-CERT-REGRESSION-001：原格式回归

- 输入：现有单 PEM、单 DER、历史记录 fixture。
- 预期：导入、持久化、恢复和 Socket TLS 行为不变。
- 证据：`docs/testing/evidence/<执行日期>/TASK-20260825-005/LEGACY-CERT-REGRESSION-001/`。

### MCP-OPENSSL-GUIDANCE-001：MCP 指令

- 输入：内嵌 `certificate-concepts` 资源。
- 预期：包含本任务的四组 OpenSSL 指令；MCP capability、工具清单和运行状态零变化。
- 证据：`docs/testing/evidence/<执行日期>/TASK-20260825-005/MCP-OPENSSL-GUIDANCE-001/`。

### LIVE-FIRSTDATA-TLS-001：真实后台分层验收

- 输入：归档 Bundle、`195.160.171.102:63002`、实现后的 Socket Listener 配置。
- 分别记录：TCP 建连、TLS 协商、信任材料加载数量、证书链验证、hostname/IP 验证、客户端证书请求、
  最终握手。
- 通过条件：Proxy 确实加载两张 CA，证书链不再因缺少 Intermediate 失败。若后续独立失败为 hostname
  或客户端身份，必须保存原始输出并准确归类，不得把该层描述为 Bundle 失败或完整业务成功。
- 证据：`docs/testing/evidence/<执行日期>/TASK-20260825-005/LIVE-FIRSTDATA-TLS-001/`。

### 推荐定向命令

实现时根据最终测试名补全过滤器，Cargo 命令统一从仓库根目录执行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml \
  -p intercept-proxy-infrastructure listener_certificates

cargo test --manifest-path src-tauri/Cargo.toml \
  -p intercept-proxy-runtime socket_relay::tests::tls
```

随后执行受影响 crate 全量、Rust fmt/Clippy、前端定向测试和现有架构门禁；实际命令、输入、输出和
复测方式必须保存到对应证据目录。

## 对抗审查计划

- T1-T3 涉及证书解析、持久化和 TLS，计划执行小任务针对性审查。
- T4-T5 是否执行小任务审查按实际 diff 和风险判断，并在任务记录中说明。
- 全部小任务完成后必须由未实现该任务的独立 Agent 执行整体审查。
- 审查覆盖需求符合度、旧格式兼容、单证书截断根因、错误传播、TLS 验证强度、存储往返、测试证据、
  文档和 diff 一致性。
- 最终结论必须为 `APPROVE`；P0、P1 和范围内 P2 全部关闭后才可归档。

## 文档影响分析

| 文档 | 影响判断 | 计划操作 |
| --- | --- | --- |
| `.gitignore` | 需要更新 | 只允许任务证据 `resources` 目录归档 PEM，其他 PEM 继续忽略 |
| `docs/README.md` | 需要更新 | 当前登记 pending 入口；完成后移除 |
| `docs/tasks/README.md` | 需要更新 | 完成后增加任务索引 |
| `docs/user-operation-guide.md` | 需要更新 | 增加单文件多 CA PEM Bundle 导入说明 |
| `docs/requirements.md` | 需要检查 | 若现有上游信任格式描述为单证书则更新 |
| `docs/architecture/security-and-persistence.md` | 需要更新 | 记录 Bundle 持久化、恢复和旧记录兼容边界 |
| `docs/mcp/certificate-concepts.md` | 需要更新 | 增加 OpenSSL 指令和错误分层 |
| `docs/mcp/tool-reference.md` | 无需更新 | 不新增或修改 MCP Tool |
| `docs/testing/release-validation-matrix.md` | 需要更新 | 增加上游多 CA Socket TLS 回归 |
| 根 README、ADR、外部包、Android 文档 | 需要检查 | 无产品合同变化则记录无需更新 |

## 实施记录

### 2026-08-25 20:44:12 +08:00

- 使用 Skill：`documentation-and-adrs`。
- 来源：本地 Skill `/Users/codin/.agents/skills/documentation-and-adrs/SKILL.md`。
- 替代步骤：任务结构、确认记录、设计取舍和文档影响分析。
- 保留门禁：任务 ID、pending 日期目录、完整测试资源、证据索引、零假设、Git/CI 和归档规则。
- 当前源码核对：上游导入仍调用单证书解析，Socket/Reverse 运行时已使用证书集合。
- 资源验证：OpenSSL 3.6.3 能解析两张证书，Intermediate 对 Root 验证为 `OK`。
- 结果：需求已登记，测试资源已准备；生产实现和 Proxy 功能测试尚未开始。

### 2026-08-25 20:50:52 +08:00

- 两张归档证书与用户提供文件逐字节一致。
- `.gitignore` 仅对 `docs/testing/evidence/**/resources/*.pem` 开放归档，其他目录的 PEM 继续忽略。
- 使用归档证书通过 OpenSSL 管道生成的 Bundle 成员数量为 `2`。
- Intermediate 对 Root 的验证结果为 `OK`。
- 任务 ID 唯一、pending 路径和证据链接存在、JSON 可解析、旧任务路径已移除。
- 登记阶段验证：PASS；生产实现和 Proxy 功能测试仍为 NOT_RUN。

### 2026-08-26 10:33:42 +08:00

- 用户明确最终数据面绕过 `tangodev.nuvei.com:9081` 直连真实后台。
- 当前分层探测：`tangodev` TLS 成功，但 HTTP 和标准 HTTPS Proxy CONNECT 均超时，不能按标准
  HTTPS forward proxy 配置；真实后台使用完整 Bundle 的 TLS 1.3 握手和证书链验证成功。
- 单 Intermediate 失败为缺 Root；完整 Bundle 对 IP 启用名称校验时独立失败为 IP mismatch。
- 当前选中 Workspace 是 `HTTP + Socket 规则 E2E`；已有 `Shift4 TLS Smoke 20260825` 仅含默认禁用
  HTTP Listener，尚未配置真实后台，未直接写 SQLite。
- CI：未执行；本任务禁止自动触发 CI。

### 2026-08-26 10:43:54 +08:00

- 用户收窄范围：不修改 Proxy 源码，只用 Python 环境测试 TLS 握手并给出证书配置。
- 已撤回本轮临时加入的 Rust 红灯测试和 Bundle 解析草稿；未覆盖工作区原有改动。
- 使用 `examples/external-packages/au_eftex/.venv/bin/python`、Python 3.14.7、OpenSSL 3.6.3，
  在内存中加载 `sub.pem + DigiCertCA.pem`，直连 `195.160.171.102:63002` 成功完成 TLS 1.3。
- 证书链验证 PASS；严格 IP 名称校验按预期失败为 verify code 64；未发送业务报文。
- TASK-20260825-005 产品实现继续保持 `待实现`；本轮只提供 Python 配置，不声称 Proxy 已支持多 CA。

### 2026-08-27 14:45:00 +08:00 至 15:27:02 +08:00

- 上游信任 PEM 改为严格解析全部 `CERTIFICATE` 成员，保持输入顺序并生成稳定规范化 Bundle；任何成员
  解析或 CA 角色失败时整体拒绝。
- 托管证书、可移植导出、恢复和环境候选验证均保存并恢复完整 Bundle；旧 PEM、DER 和单成员记录无需迁移。
- Socket TLS 显式信任库加载全部成员，并启用 OpenSSL partial-chain 验证，使显式导入的 Intermediate
  能补齐服务端未发送的链；系统信任路径未改变。
- 本地真实 TLS 回归证明：服务端只发送叶子证书时，Root-only 失败，Intermediate + Root Bundle 成功；
  单证书、客户端身份、下游 TLS/mTLS 和 Reverse TLS 回归保持通过。
- UI 明确继续使用单文件导入，一个 PEM 文件可包含一个或多个 CA；MCP 证书指南补充组合、列举、验证
  和真实握手指令，不新增文件写入或配置工具。
- 真实 First Data 目标完成 TLS 1.3 握手，证书链验证为 OK，未发送业务报文。
- 主 Agent 完成针对解析完整性、存储往返、运行时验证强度、旧格式兼容、文档边界和证据一致性的
  对抗复核，未发现未关闭阻断；当前编排限制下未额外启动独立审查 Agent，此限制已如实记录。

## 修改文件

### 登记阶段

- `.gitignore`
- `docs/README.md`
- `docs/tasks/pending/2026-08-25/upstream-multi-ca-pem-bundle.md`
- `docs/testing/evidence/README.md`
- `docs/testing/evidence/2026-08-25/TASK-20260825-005/TLS-CA-BUNDLE-001/`

### 实现阶段

- `src-tauri/crates/infrastructure/src/certificates.rs`
- `src-tauri/crates/infrastructure/src/certificates/trust_anchor.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_certificates.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_certificate_portable.rs`
- `src-tauri/crates/infrastructure/src/adapters/environment_configuration_validation.rs`
- `src-tauri/crates/proxy/src/socket_relay/upstream_tls.rs`
- `src-tauri/crates/proxy/src/socket_relay/tests/tls/probes.rs`
- `src-tauri/crates/infrastructure/src/adapters/listener_certificates_tests/bundles.rs`
- `src/features/listeners/fixed-server-tls-import-modals.tsx`
- `src/features/listeners/fixed-server-tls-import-modals.test.tsx`
- `docs/user-operation-guide.md`
- `docs/mcp/certificate-concepts.md`
- `docs/architecture/security-and-persistence.md`
- `docs/requirements.md`
- `docs/testing/release-validation-matrix.md`

## 附加文件

- 测试资源与复测入口：
  `docs/testing/evidence/2026-08-25/TASK-20260825-005/TLS-CA-BUNDLE-001/`
- 原始证书：`resources/sub.pem`、`resources/DigiCertCA.pem`。
- 目标配置：`inputs/backend.json`。
- 复测步骤：`steps/replay.md`。
- 当前资源验证输出：`outputs/resource-validation.txt`。
- 最终验收：`docs/testing/evidence/2026-08-27/TASK-20260825-005/TLS-CA-BUNDLE-FINAL-001/`。

## 测试结果

- 任务登记、资源完整性和复测入口：PASS。
- 资源解析与 Intermediate/Root 关系：PASS。
- Proxy 多证书解析：`3/3 PASS`。
- Listener 证书持久化和恢复：`12/12 PASS`；证书相关组 `55/55 PASS`。
- 环境候选证书验证：`11/11 PASS`。
- Socket TLS Trust Store 与 partial-chain：`9/9 PASS`；Reverse 上游 TLS `2/2 PASS`。
- 前端全量：`67` 个文件、`659` 项 `PASS`。
- 真实后台：TLS 1.3、`TLS_AES_256_GCM_SHA384`、Peer `test Axium`、链验证 `OK`，业务报文 `0` 字节。
- 格式、源码规模、严格 Clippy 和相关架构门禁：`PASS`。
- CI：由 `TASK-20260827-003` 在最终整合推送后统一触发和确认。

## 验收结果

- 当前阶段：实现、复测、文档与证据归档完成。
- 功能验收：`PASS`。
- 对抗审查：`PASS`，无未关闭阻断；独立 Agent 限制如实施记录所述。
- 最终整合 Windows CI 由 `TASK-20260827-003` 统一管理，不阻塞本功能任务归档。

## 文档同步结果

用户操作、需求、TLS/持久化架构、MCP 证书指南和发布验证矩阵已按实际实现同步。

## 完成总结

- 一个上游信任文件现在可以严格承载多个 CA，并在规范化、受保护持久化、恢复、环境验证和 Socket
  TLS 运行时之间完整保持成员集合。
- 真实后台与本地缺失 Intermediate 场景均证明 Bundle 能补齐证书链，同时没有放宽 hostname、客户端
  身份或业务报文边界。
- 旧单证书格式继续通过，无数据库迁移、无多文件选择器、无 MCP 写能力扩张。

## 停止条件

本任务在实现、分层复测、真实 TLS 验证、文档同步、证据归档和任务索引一致后停止；最终整合提交、
推送与 Windows CI 继续由 `TASK-20260827-003` 执行。
