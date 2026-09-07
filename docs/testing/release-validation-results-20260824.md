# Intercept Proxy 发布级验证结果（2026-08-24）

## 1. 结论

- 当前结论：`BLOCKED`。
- 自动化门禁、HTTP/Socket 核心 loopback、TLS/mTLS 组合测试、内部协议包、Deno/Python
  外部包合同、AU EFTEX 历史报文 DUKPT 往返均已通过。
- 尚不能宣称“全部生产场景 PASS”：本次没有真实远端 Server/厂商证书与真机密钥，也没有厂商
  DE64/DE128 MAC 生成合同；HTTP CONNECT、HTTP Upgrade、Forward HTTPS 和 HTTP 外部软件包
  是当前产品明确不支持的能力，只验证预期拒绝。
- 可复用的完整测试内容、固定端口、执行顺序和通过标准见
  `docs/testing/release-validation-matrix.md`。下次执行不得只复用本结果，必须重新运行该规范。

## 2. 本次源码与环境

- 仓库：`/Users/codin/Code/gmofg-payment-proxy`
- Cargo manifest：`src-tauri/Cargo.toml`
- release App：`src-tauri/target/release/bundle/macos/Intercept Proxy.app`
- E2E Workspace：`HTTP + Socket 规则 E2E`
- 数据面全部绑定 `127.0.0.1`，Mock Server 也运行在本机。
- HTTP：Proxy `18080`，Server `19080`。
- Socket Scripted：Proxy `18081`，Server `19081`。
- Socket Direct：Proxy `18082`，Server `19082`。
- AU EFTEX 历史 trace：
  `/Users/codin/WxWork/WXWork Files/Caches/Files/2026-08/ebcb7bd174edc760b9b298db8f0de8d6/Internal transaction trace20261806_1252.txt`

## 3. 自动化门禁

| 检查 | 结果 | 证据摘要 |
| --- | --- | --- |
| TypeScript typecheck | PASS | `pnpm typecheck` |
| ESLint | PASS | `pnpm lint` |
| 架构扫描 | PASS | `pnpm scan:architecture` |
| 文件大小扫描 | PASS | `pnpm scan:source-size` |
| Frontend 单元/UI 合同 | PASS | 66 files，647 tests |
| Next production build | PASS | `pnpm build` |
| macOS release App | PASS | ad-hoc 签名 `.app` 构建成功，并实际启动冒烟通过 |
| Rust fmt | PASS | workspace fmt check |
| Rust clippy | PASS | workspace/all-targets/all-features，`-D warnings` |
| Rust workspace tests | PASS | 全 workspace 测试无失败 |
| 覆盖率策略 | PASS | Frontend 覆盖率门禁基线已通过；Rust 各 package 与逐文件阈值全部通过 |
| Deno ISO8583 | PASS | fmt、lint、14 tests |
| AU EFTEX Python | PASS | 68 tests |
| Python E2E/trace runner tests | PASS | 19 tests |

覆盖门禁首轮不是业务失败：基础设施 464 个测试全部通过，但策略正确发现
`external_relay/contract.rs` 与 `external_relay/diagnostics.rs` 的行为分支缺少回归证明。前者已补到
Functions 100%、Lines 100%、Regions 98.87%，后者补到 Functions 100%、Lines 95.83%、
Regions 91.89%。清理重构后已删除文件的旧覆盖清单后，全量覆盖门禁重新执行并通过。

## 4. HTTP 验证

| 场景 | 层级 | 结果 | 实际证据 |
| --- | --- | --- | --- |
| Fixed Server HTTP | L3 | PASS | POST `/e2e/http` 完成 App→Proxy→Server→Proxy→App |
| 请求 JSON 规则 | L3 | PASS | amount `111 → 222`，请求 Header 被设置 |
| 响应 JSON/状态规则 | L3 | PASS | amount 改为 `333`，状态码改为 `209` |
| 四方向抓包与即时刷新 | L4 | PASS | 不切页连续执行两次，列表立即追加记录 |
| Forward absolute-form HTTP | L2 | PASS | `raw_http_proxy`/`forward_proxy` 集成测试 |
| CONNECT | L2 | PASS（预期拒绝） | 返回 `501`，不拨号上游 |
| Upgrade/WebSocket | L2 | PASS（预期拒绝） | 返回 `501`，不建立旁路升级 |
| downstream TLS/mTLS | L2 | PASS | `tls_mtls` 6 tests，成功与错误身份拒绝 |
| upstream HTTPS/mTLS | L2 | PASS | `upstream_http` 7 tests，CA/hostname/client identity |
| 双侧 TLS/mTLS | L2 | PASS | Rust 真实 loopback 证书握手与 Body 往返 |
| HTTP 条件与动作族 | L1/L2 | PASS | domain/application/proxy 全量规则测试与非法配置拒绝 |
| HTTP 阶段能力编辑器 | L2/L4 | PASS | Rust 能力矩阵驱动；打包 App 中请求/响应/TLS 选项互斥 |
| HTTP Body Protocol | L1/L2 | PASS | 内部 ZIP/Rhai Decode/Rules/Encode 合同 |
| HTTP 外部 WebSocket 软件包 | L1 | PASS（预期拒绝） | 当前产品只允许 Socket 外部包 |

HTTP L3 使用代表性规则链证明真实 wire 行为；条件和故障动作的穷举放在 L1/L2，避免把延迟、
断开、错误 Content-Length 等故障注入做无意义的笛卡尔积。

规则编辑器另发现并修复了“所有阶段展示全部动作”的能力泄漏。最终打包 App 实际确认：

- 请求阶段不显示自定义 HTTP 状态码、非法 JSON、错误长度、截断和下行断连；
- 响应阶段不显示 Mock、上游 connect/write/read timeout、丢弃上游响应和上游 Body 断连；
- TLS 空白规则只能选择证书指纹/第 N 次命中，添加动作默认得到“拒绝 TLS 握手”；
- 限速/间歇方向由阶段固定，终止动作后“添加动作”禁用；
- 旧草稿切换到不兼容阶段时明确提示，不允许用空白选择掩盖非法动作；
- Rust 保存校验继续拒绝跨阶段动作、错误方向、多个终止动作和终止动作非末尾。

## 5. Socket 验证

| 场景 | 层级 | 结果 | 实际证据 |
| --- | --- | --- | --- |
| RemoteServer Direct | L3 | PASS | 上行 32771 B、下行 16387 B，逐字节一致，半关闭保持 |
| RemoteServer Scripted | L3 | PASS | 分段 ISO8583 Frame 后立即发送，不等待第二帧 |
| Scripted 四阶段规则 | L3/L4 | PASS | request `1000→1111→2222`；response `2222→3333→4444` |
| 多 Envelope/顺序 | L2/L4 | PASS | Exchange 内按实际顺序追加，D2 不覆盖 D1 |
| 不完整 Frame | L3/L4 | PASS | 单字节 `00`，0 B 转发，`PROCESSING_FAILED/FrameProcess` |
| LocalServer Direct | L2 | PASS | echo，无虚假 upstream connect |
| LocalServer Scripted | L2 | PASS | 真实 IPC/SQLite/Rhai/TCP/capture 跨层测试 |
| upstream TLS/mTLS | L2 | PASS | TCP→TLS/mTLS 成功与错误信任失败 |
| downstream TLS/mTLS | L2 | PASS | TLS/mTLS→TCP 与 LocalServer 身份策略 |
| 双侧 TLS/mTLS | L2 | PASS | TLS→TLS 双侧认证与关闭流程 |
| 连接失败/EOF/reset/timeout | L1/L2 | PASS | runtime/proxy failure-path tests |

发现并修复一项展示语义缺陷：Scripted Listener 曾因 TCP 传输被显示为
`Socket · Transparent`。现在 Listener 明确显示“按协议转发/透明转发”，TCP/TLS 仅作为传输
后缀；诊断日志也使用“传输：TCP → TLS”，不再把 transport 当作 processing。

## 6. 协议包、外部软件包与 DUKPT

| 场景 | 层级 | 结果 | 实际证据 |
| --- | --- | --- | --- |
| 内置 ISO8583 Rhai | L2/L3 | PASS | Frame/Decode/Display/Rules/Encode 与 59 B 真实报文 |
| Deno ISO8583 | L1/L2/L3/L4 | PASS | 14 tests；release App 真实 listener 59 B 双向逐字节通过；断线停止精确 listener，重连不自动恢复 |
| 外部 JSON-RPC 合同 | L1/L2 | PASS | 上下行 hook、错误传递、超时、规则热替换 |
| AU EFTEX Python | L1/L2/L3/L4 | PASS | 68 tests；公开 DUKPT request 71 B / response 63 B 经真实 listener 双向逐字节通过；断线/重连 fail-closed |
| AU EFTEX 历史报文 | L2 | PASS | request 775 B/MTI 1200；response 887 B/MTI 1210 |
| DUKPT | L1/L2 | PASS（限定） | IPEK/key/data variant、两方向解密、重加密 wire round-trip |
| 厂商 MAC | 外部条件 | BLOCKED | 缺少 DE64/DE128 生成/替换合同和真实生产密钥 |

历史 trace 新鲜执行的断言均为 true：DUKPT 派生、分段 Frame、request/response Document
round-trip、RPC round-trip、wire round-trip。带 MAC 报文字段修改仍按设计返回
`MAC_REPLACEMENT_REQUIRED`；observe-only 精确往返通过，不能描述成完整支付 MAC 验证。

AU EFTEX CLI 的 Ctrl-C 生命周期也已修正：`asyncio.run` 完成取消和 WebSocket 清理后，入口将
`KeyboardInterrupt` 视为正常的运维退出，不再打印 traceback 或返回退出码 1；回归测试和真实
Ctrl-C 退出码均通过。

## 7. 观测与失败路径

- 成功 Socket Exchange 按序显示连接、App→Proxy、Proxy→Server、Server→Proxy、Proxy→App、完成。
- Display HTML 以表格渲染；不再渲染 Document JSON；字节区没有无意义的上一/下一字节页按钮。
- 页面停留在抓包列表时，第二次交易立即追加，无需切换页面。
- 不完整 Frame 的失败交易仍立即生成记录，保留输入字节、失败阶段和稳定错误码。
- 观测错误不改变已成功业务结果；Frame/Decode/Rules/Encode 等业务流水线错误 fail-closed。

## 8. 未支持边界与剩余外部验收

下列项目不伪装为成功能力：

1. HTTP Forward HTTPS/CONNECT MITM 当前不支持；只接受明确 `501` 且不拨号。
2. HTTP Upgrade/WebSocket 当前不支持；只接受明确 `501`。
3. HTTP 外部 WebSocket 协议包当前不支持；外部软件包为 Socket-only。
4. 没有真实远端 Server、厂商 CA/client identity、Android 真机和生产 DUKPT/MAC 材料，因此本次只能
   证明本机 release App loopback、真实 TLS 栈和历史 trace，不能证明外部网络环境。

Deno 与 AU EFTEX 已通过 release App 的精确版本绑定、真实 Socket listener、真实 Mock Server/App
完整数据面交易；这证明本机公开测试向量合同，不替代真实厂商 Server、生产 BDK 或 MAC 验收。

外部包 JSONL 证据边界新增 6 个回归测试：固定 Deno/AU EFTEX 顺序、只导出白名单字节计数、
拒绝负数或非整数计数、禁用包安装失败不污染 Workspace、恢复不存在 Workspace 不改变当前选择、
fixture 被篡改时在网络 I/O 前失败。测试先复现了 payload 泄漏、负计数和非稳定顺序，再收紧 writer。

## 9. 下次复用步骤

1. 先读取 `release-validation-matrix.md`，复制本文件为当天结果文件。
2. 确认固定端口未占用；不得自动杀死未知进程。
3. 依次执行静态门禁、Frontend、Rust、Deno、Python、覆盖率。
4. 按 `release-validation-matrix.md` 的固定命令安装 External Packages E2E Workspace，启动 Deno 与
   AU EFTEX 后运行 `python3 scripts/e2e_external_packages.py run`；逐包断线/重连后执行
   `assert-stopped`。
5. 核对成功与失败 Exchange 都能实时追加，并记录四方向字节/字段证据。
6. 若有真实厂商环境，再追加远端 TLS/mTLS、真机和 MAC 验证，不覆盖本地结果。
7. 关闭 Mock Server/外部包/App，验证所有端口可重新绑定。
8. 使用 `install` 输出的 `previous_selected_id` 执行 `restore-selection`，恢复原选中 Workspace。
9. 最后重新构建 `.app`；任何修复后都必须从受影响层开始重跑，不能沿用修复前结果。
