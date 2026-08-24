# Intercept Proxy 发布级全功能验证规范

## 1. 目的与复用方式

本文是 HTTP、Socket、规则、协议包、外部软件包和 TLS 能力的固定验收合同。每次大型重构、协议能力修改或发布前，都按本文执行，并把结果写入同目录的
`release-validation-results-YYYYMMDD.md`。

验证目标不是简单证明“测试命令返回 0”，而是分别证明：

1. 配置能够保存、加载和启动。
2. App 到 Proxy、Proxy 到 Server、Server 到 Proxy、Proxy 到 App 的真实字节或消息语义正确。
3. Reader、Frame、Decode、Display、Rules、Encode 和 Writer 按设计顺序执行。
4. HTTP 与 Socket 的规则、协议包和传输安全边界互不串用。
5. 失败路径 fail-closed，并在抓包/诊断中保留可定位证据。
6. release App 与测试所覆盖的源码一致。

## 2. 验收等级与通过条件

| 等级 | 证明内容 | 最低证据 |
| --- | --- | --- |
| L1 | 纯领域约束、规则和编解码 | Rust/TypeScript/Python/Deno 单元测试 |
| L2 | 真实模块组合、SQLite、协议包运行时 | Rust/Python 集成测试，使用真实 TCP 或临时文件 |
| L3 | 真实 Proxy 数据面 | release App + loopback App/Server 客户端，逐字节或逐字段断言 |
| L4 | 原生 UI 与观测 | App 中 Listener 状态、规则命中、Exchange 四方向记录、错误事件 |

总结果只有以下三种：

- `PASS`：本次范围内所有必测项通过。
- `BLOCKED`：只有无法由仓库或本机提供的外部条件阻塞，并记录具体缺失物和已完成的替代验证。
- `FAIL`：任何业务流水线、字节保真、规则、TLS、协议包或观测合同不符合预期。

观测失败本身不应阻断交易，但观测缺失仍使对应 L4 项失败；业务流水线失败必须使交易失败，禁止静默透明转发。

## 3. 固定环境

- 仓库根目录：`/Users/codin/Code/gmofg-payment-proxy`
- Cargo manifest：`src-tauri/Cargo.toml`
- 手工 App 数据库：`~/Library/Application Support/com.interceptproxy.desktop/intercept-proxy.sqlite3`
- 自动发布验收必须使用临时 App profile/SQLite；在隔离入口实现前，只允许先备份、再幂等写入固定 E2E Workspace，并在结束时恢复原 selected Workspace。
- E2E Workspace：`HTTP + Socket 规则 E2E`
- 测试只绑定 `127.0.0.1`，真实远端验证另行记录。
- 安装 E2E Workspace 前必须停止 App；脚本先备份数据库，再幂等更新专用 Workspace。
- TLS 证书和私钥使用运行时生成的临时测试材料，不写入源码，不复用生产身份。

固定基础端口：

| 用途 | Proxy | Mock Server |
| --- | ---: | ---: |
| HTTP fixed Server | 18080 | 19080 |
| Socket scripted ISO8583 | 18081 | 19081 |
| Socket direct transparent | 18082 | 19082 |
| HTTP forward proxy | 18083 | 动态 |
| HTTP downstream HTTPS / mTLS | 18443 / 18444 | 19443 / 19444 |
| Socket LocalServer direct / scripted | 18085 / 18086 | 不适用 |
| Socket upstream TLS / mTLS | 18087 / 18088 | 19087 / 19088 |
| Socket downstream TLS / mTLS | 18487 / 18488 | 19487 / 19488 |

若端口被占用，测试脚本应在启动前失败并报告占用进程，不得自动杀死未知进程。

## 4. HTTP 功能矩阵

### 4.1 数据面与服务模式

| ID | 模式 | App → Proxy | Proxy → Server | 必须断言 |
| --- | --- | --- | --- | --- |
| H-FIXED-PLAIN | Fixed Server | HTTP | HTTP | method、path/query、headers、body 和响应完整 |
| H-FORWARD-PLAIN | Forward Proxy | absolute-form HTTP | HTTP | authority 解析正确，不逃逸到固定 Server |
| H-FORWARD-CONNECT | Forward Proxy CONNECT | CONNECT | 不连接目标 | 当前 Exchange 合同明确返回 501，且不得建立旁路隧道 |
| H-FORWARD-UPGRADE | Forward Proxy Upgrade | Upgrade | 不连接目标 | 当前 Exchange 合同明确返回 501，且不得升级 WebSocket |
| H-DOWN-TLS | Fixed Server | HTTPS | HTTP | App 校验 Server 身份，普通 TLS 成功 |
| H-DOWN-MTLS | Fixed Server | HTTPS + client cert | HTTP | 必需客户端证书成功；缺失/错误证书拒绝 |
| H-UP-TLS | Fixed Server | HTTP | HTTPS | CA 和 hostname 校验成功；错误 hostname/CA 拒绝 |
| H-UP-MTLS | Fixed Server | HTTP | HTTPS + client cert | Proxy 客户端身份成功；缺失/错误身份拒绝 |
| H-BOTH-MTLS | Fixed Server | HTTPS + client cert | HTTPS + client cert | 两侧 mTLS 同时成功，Body 字节保持 |

这里的 CONNECT/Upgrade 是明确的“不支持合同”，不是待实现成功路径。不得用 reverse HTTPS 或普通 TLS 测试冒充 CONNECT MITM。

### 4.2 Body 处理

| ID | Body | 编解码模式 | 必须断言 |
| --- | --- | --- | --- |
| H-BODY-JSON | JSON object/array/nested | plain auto | 嵌套 JSONPath、数值/字符串/布尔/null 保持 |
| H-BODY-TEXT | UTF-8 text | plain text | ReplaceBodyText 和 Content-Length 正确 |
| H-BODY-XML | XML text | plain text | 未配置协议包时不错误解释或改写 |
| H-BODY-HEX | arbitrary bytes | plain/binary | 未命中规则时逐字节保持 |
| H-BODY-PROTOCOL | protocol Document | protocol package | Decode → Rules → Encode；无修改时不重写 |

HTTP Body Protocol 当前只支持内部 ZIP/Rhai 协议包；外部 WebSocket 软件包当前是 Socket-only。该限制必须有配置拒绝测试，不能把“不支持”记录为跳过。

### 4.3 HTTP 条件穷举

每个字段至少分别验证 `Equals`、`Contains`、`Regex` 的命中和不命中：

- Terminal IP
- Certificate fingerprint
- Path / request type
- JSONPath，包括嵌套对象和数组索引
- NthHit：命中前、目标次数、目标次数之后
- channel、stage、priority、created order、enabled、one-shot

### 4.4 HTTP 动作穷举

下列每个动作都必须有合法输入成功测试、边界值测试和非法配置拒绝测试：

- `SetJsonField`
- `ReplaceBodyText`
- `SetHeader`
- `Delay`
- `Jitter`：before-message / per-chunk
- `Throttle`：upstream / downstream
- `Intermittent`：upstream / downstream
- `Pause`
- `CustomHttpStatus`
- `RejectTlsHandshake`
- `DisconnectBeforeUpstream`
- `UpstreamConnectTimeout`
- `UpstreamWriteTimeout`
- `UpstreamReadTimeout`
- `DropUpstreamResponse`：read-complete / close-after-request-write
- `MockResponse`
- `InvalidJson`
- `IncorrectContentLength`
- `TruncateResponse`
- `DisconnectDuringUpstreamWrite`
- `DisconnectDuringDownstreamWrite`

动作链额外验证声明顺序、终止动作后不再执行、总延迟和流量参数上限。

## 5. Socket 功能矩阵

### 5.1 拓扑、处理和安全组合

| ID | Endpoint | Pipeline | App 侧 | Server 侧 | 必须断言 |
| --- | --- | --- | --- | --- | --- |
| S-RELAY-DIRECT | RemoteServer | Direct | TCP | TCP | 任意二进制双向逐字节保持、半关闭保持 |
| S-RELAY-SCRIPT | RemoteServer | Scripted | TCP | TCP | Frame → Decode → Display → Rules → Encode 四阶段 |
| S-LOCAL-DIRECT | LocalServer | Direct | TCP | 无连接 | 收到什么回复什么；不产生虚假 upstream connect |
| S-LOCAL-SCRIPT | LocalServer | Scripted | TCP | 无连接 | upstream 文档经本地响应进入 downstream Pipeline |
| S-UP-TLS | RemoteServer | Direct/Scripted | TCP | TLS | CA、SNI/hostname 成功；错误信任失败 |
| S-UP-MTLS | RemoteServer | Direct/Scripted | TCP | mTLS | Proxy 客户端身份成功；缺失/错误身份失败 |
| S-DOWN-TLS | RemoteServer/LocalServer | Direct/Scripted | TLS | TCP/无连接 | App 侧 Server 身份成功 |
| S-DOWN-MTLS | RemoteServer/LocalServer | Direct/Scripted | mTLS | TCP/无连接 | required/optional/disabled 三种策略 |
| S-BOTH-MTLS | RemoteServer | Direct/Scripted | mTLS | mTLS | 两侧握手、数据和关闭顺序完整 |

Direct 模式只验证 transport，不调用 Frame/Decode/Display/Rules/Encode。Scripted 模式任何业务阶段失败都必须关闭当前 Exchange，禁止降级到 Direct。

### 5.2 Frame 和连接生命周期

- 一次 read 不足一帧：返回 `NeedMore`，不连接/写入 Server。
- 完整一帧：立即处理并发送，不等待第二帧。
- 同一 TCP 连接连续多帧：每帧创建独立 Envelope，按序处理，D2 不覆盖 D1。
- frame + 下一帧前缀粘包：只消费当前帧字节，余量留给下一次。
- EOF、reset、App 断开、Server 断开、读写超时：关闭 Exchange 并记录最终事件。
- 同一 Exchange 内严格执行 request → response 配对；当前明确不支持多个并发在途请求。

### 5.3 Document 规则穷举

- 四阶段：app_to_proxy、proxy_to_upstream、upstream_to_proxy、proxy_to_app。
- 条件：字段严格类型 `Equals`；string、int、bool、blob 分别命中/不命中。
- 多条件 AND、空条件恒匹配、priority、created order、disabled。
- 动作：RecordMatch、SetField、ClearField、ClearDocument。
- 多规则顺序累积；每次阶段从前一阶段产生的新 Document 继续执行。
- 修改后必须 Encode；未修改时保持原始 wire bytes。
- schema、package version、listener 或 direction 不匹配时 fail-closed。

## 6. 协议包与外部软件包矩阵

| ID | 实现 | 能力 | 必须断言 |
| --- | --- | --- | --- |
| P-RHAI-ISO8583 | 内置 Rhai | Frame/Decode/Display/Encode | 分段、粘包、四方向规则、无修改字节保持 |
| P-DENO-ISO8583 | 外部 Deno | WebSocket JSON-RPC | 注册、上下行 hook、断线、重连、超时、限额 |
| P-PY-AU-EFTEX | 外部 Python | H01 + DUKPT + ISO8583 | 两方向派生、解密、Document、Display、重加密逐字节一致 |
| P-HTTP-BODY | HTTP Body 包 | Decode/Display/Encode | request/response 独立 schema 和规则阶段 |

所有包共同验证：

- manifest、document schema、入口存在性、Rhai/JSON-RPC 合同和精确版本绑定。
- 启用、禁用、离线、重连、替换版本、删除占用包。
- frame/decode/display/rules/encode 调用顺序和超时预算。
- 包错误、无效 Base64、错误 response id、超限消息、断线全部 fail-closed。
- Display 失败只影响观测时，不影响已经成功的业务流水线；业务 hook 失败必须终止交易。

### 6.1 AU EFTEX / DUKPT 特殊验收

- 公开合成向量：IPEK、transaction key、request/response Data key、3DES-OFB、动态 IV、padding。
- 外部历史 trace：request/response wire round-trip、313 字节分段、三种长度前缀模式。
- H01、KSN、ISO8583 profile 和上下行方向错误均有稳定错误码。
- 带 DE64/DE128 的报文仅允许 observe-only；字段变化必须返回 `MAC_REPLACEMENT_REQUIRED`。
- 在厂商 MAC 合同缺失时，不得把“DUKPT 加解密通过”描述为“完整支付 MAC 验证通过”。

## 7. 观测与 UI 验收

每个成功 Exchange 至少出现按实际顺序追加的四方向事件：

1. App → Proxy 收到
2. Proxy → Server 发送
3. Server → Proxy 收到
4. Proxy → App 发送

LocalServer 不伪造 Server 网络连接，但仍通过同一 upstream/downstream Pipeline 显示请求与响应。失败交易必须显示已发生的输入/输出和失败阶段；刷新、页面切换和 WebSocket 推送不得遗漏或覆盖记录。

HTTP 与 Socket 抓包、规则、协议包均使用统一列表，不以 Tab 隐藏另一个协议；详情只显示固定的 Display HTML，不渲染 Document JSON。

## 8. 执行顺序与固定命令

真实 loopback 不做全部轴的笛卡尔积：每个拓扑保留一个完整成功锚点，每个 TLS/mTLS 信任边界至少一个成功和一个失败；规则类型和动作类型在 L1/L2 穷举，L3 使用代表性动作族并验证真实 wire 行为。预计核心真实网络集 35–50 例。

### 8.1 静态和单元/集成门禁

```bash
pnpm scan:architecture
pnpm scan:source-size
pnpm lint
pnpm typecheck
pnpm test:ui-contracts
pnpm test
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --workspace
examples/external-packages/au_eftex/.venv/bin/python -m unittest \
  scripts/test_e2e_proxy_rules.py scripts/test_verify_au_eftex_trace.py
deno task --cwd examples/external-packages/iso8583-deno check
examples/external-packages/au_eftex/.venv/bin/python -m unittest discover \
  -s examples/external-packages/au_eftex/tests -v
```

### 8.2 release App 真实 loopback

```bash
python3 scripts/e2e_proxy_rules.py install
# 启动 release App，并启动测试 Workspace 的 Listener
python3 scripts/e2e_proxy_rules.py run
```

外部软件包使用独立、固定的 E2E Workspace，避免把注册生命周期证据误当成数据面证据：

```bash
# App 必须已停止；命令拒绝占用端口，只新增/替换固定 E2E Workspace，且先备份 SQLite。
python3 scripts/e2e_external_packages.py install

# 启动 release App。分别启动以下真实进程，并在“入口配置”启动 18083、18084：
EXTERNAL_PACKAGE_URL=ws://127.0.0.1:8765/packages \
  deno task --cwd examples/external-packages/iso8583-deno start
AU_EFTEX_BDK_HEX=0123456789ABCDEFFEDCBA9876543210 \
EXTERNAL_PACKAGE_URL=ws://127.0.0.1:8765/packages \
  examples/external-packages/au_eftex/.venv/bin/au-eftex

# 两条交易都必须通过真实 Socket listener 和真实 Mock Server，逐字节断言上下行结果。
python3 scripts/e2e_external_packages.py run \
  --evidence output/e2e-external-packages.jsonl

# 每个包分别执行：先保持对应 listener 运行，再停止包进程。
# 断线必须停止精确绑定的 listener 并释放端口；重启包后 listener 仍不得自行恢复。
python3 scripts/e2e_external_packages.py assert-stopped deno
python3 scripts/e2e_external_packages.py assert-stopped au-eftex

# App 停止后，使用 install 输出的 previous_selected_id 恢复原选中 Workspace。
python3 scripts/e2e_external_packages.py restore-selection \
  --workspace-id <previous_selected_id>
```

固定数据面合同：Deno 使用带 2 字节长度头的 ISO8583 `0200/0210`；AU EFTEX 使用公开 BDK/KSN
合成的 request/response DUKPT 金丝雀向量。两者均测试 TCP 分段、精确包版本绑定、真实 Frame/Decode/
Display/Encode RPC 和双向 wire 一致性。AU EFTEX 向量不含生产密钥、PAN、PIN 或真实交易数据。

扩展 TLS、LocalServer、外部包场景必须由同一 E2E runner 自动创建临时证书、Mock Server、App client，并输出 JSONL 结果；不接受只凭 UI 肉眼判断。

### 8.3 最终构建

```bash
CI=true pnpm tauri build --bundles app \
  --config '{"bundle":{"macOS":{"signingIdentity":"-","hardenedRuntime":false}}}'
```

## 9. 结果记录格式

每个测试 ID 记录：

- 状态：PASS / FAIL / BLOCKED
- 层级：L1 / L2 / L3 / L4
- 执行命令或脚本入口
- 输入摘要和预期输出
- 实际状态码、字段、字节数、规则命中数、TLS 协议与双方认证结果
- Exchange ID 或稳定关联 ID
- 日志/截图/JSON 证据路径
- 失败根因、修复提交范围和回归测试

机器记录至少包含 `run_id`、`case_id`、源码状态、各轴配置、耗时、稳定错误码、双向字节数与 SHA-256、TLS 版本/SNI/证书指纹/客户端身份提交状态、规则 ID 与阶段、Exchange 有序事件、清理和端口复绑结果。不得记录私钥、密码或完整支付报文。

## 10. 清理与幂等性

- E2E runner 只管理固定 E2E Workspace 和自己创建的临时证书、端口与进程；长期目标是全部使用临时 App profile/SQLite。
- 不删除用户 Workspace；数据库修改前保留时间戳备份。
- 若兼容模式使用用户数据库，结束时恢复原 selected Workspace；无法恢复即判定整次运行失败。
- 每次运行结束关闭 Mock Server 和外部包进程，释放端口。
- readiness 必须使用明确的监听/健康检查，不用固定 sleep；环境失败最多清理后重试一次。
- 结束后验证所有测试端口可重新绑定，且没有遗留子进程或临时密钥材料。
- 测试失败也执行清理，并保留结果 JSON、App 日志和数据库备份路径。
- 重复执行 `install` 必须更新同一 E2E Workspace，不能制造重复规则、Listener 或软件包记录。
