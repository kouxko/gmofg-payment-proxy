# TASK-20260903-005：交付 GMO-FG Payment DLL downstream Wasm 协议包

- 任务 ID：TASK-20260903-005
- 状态：已完成
- 任务日期：2026-09-03
- 创建时间：2026-09-03 15:21:33 +08:00
- 开始时间：2026-09-03 15:21:33 +08:00
- 最后更新时间：2026-09-03 16:10:00 +08:00
- 完成时间：2026-09-03 16:10:00 +08:00
- 创建路径：`docs/tasks/pending/2026-09-03/gmofg-payment-dll-downstream-package.md`
- 归档路径：`docs/tasks/completed/2026-09-03/gmofg-payment-dll-downstream-package.md`
- 关键词：`GMO-FG`、`Payment DLL`、`HTTP`、`downstream`、`Wasm Component`、`Decode`、`Encode`、`Display`、`Shift-JIS`
- 任务优先级：高
- 优先级理由：变更支付 DLL 协议、公共 Document/Encode 合同和可导入 Wasm Component；字段错位、长度错误或静默丢表会生成错误支付参数或错误业务结果，必须执行协议报文完整测试与任务级对抗审查。

## 背景、目标与历史连续性

Android Payment 工程 `/Users/codin/Code/jp_gmofg_payment` 已实现 GMO-FG DLL 请求发送和 Credit/UnionPay
响应的单向 Java 对象解析，但 Gson Table Adapter 禁止反向写出，且商户、风险和 IC 公司表中存在未建模
保留区，不能直接作为 Proxy downstream 的可逆协议包。

目标是在本仓库交付一个自包含 HTTP Wasm Component，仅对 downstream DLL 响应提供完整
Decode、Encode 和 Display：所有当前已知表按字符宽度解析，保留区可见且可编辑，Encode 自动计算
每个外层 `Length`，未知表 ID 和非法结构 fail-closed，未修改 Document 返回原始 Body。

历史连续性：

- 原任务：`TASK-20260903-001`
- 原用例：`PAYMENT-DLL-D48-MOCK-001`
- 原证据：`docs/testing/evidence/2026-09-03/TASK-20260903-001/PAYMENT-DLL-D48-MOCK-001/`
- 复用资源：该用例实际 Payment DLL `D48` downstream 响应及 Shift-JIS HTTP 配置。
- 补充来源：`/Users/codin/Code/jp_gmofg_payment` 当前源码和测试；完整 Credit 成功报文来自同产品历史工程
  `/Users/codin/Code/jp_gmofg_launcher/docs/credit.json`，使用时复制为本任务活动 fixture 和证据快照，
  不把外部绝对路径作为复测入口。

## 范围、不在范围与需求确认记录

范围：

- 新增单文件 HTTP Wasm Component 源码、Manifest、构建入口、活动 fixture、包级测试和说明文档。
- downstream 支持 `TransactionType=0000` 连接测试、`0001` Credit DLL、`0002` UnionPay DLL，以及
  `D48` 等不含参数表的错误响应。
- 支持表 ID `0`、`1`、`2`、`3`、`4`、`5`、`6`、`7`、`8`、`9`、`A`；嵌套表保持顺序、
  定长字段、空格、保留区和分隔符语义。
- Decode 校验原始 `Length`、表类型、固定宽度、重复块数量和 terminator；未知表 ID 明确失败。
- Encode 从 Document 重建所有表并自动计算 `Length`；字段宽度不合法时失败，不截断、不自动补空格。
- Display 输出 HTML 转义后的 table 视图：顶层响应、各 DLL 表、嵌套记录和保留区均按字段行展开，
  不使用 JSON `<pre>`，且不改变网络结果。
- Display 为每张 table 分配不同且稳定的安全配色，用 caption、边框和底色区分表；相同 Document 的
  表顺序与颜色保持确定性，颜色值仍经过现有安全清洗。
- 将 Proxy 的不可信 Display HTML 资源上限从 128 KiB / 4096 DOM 节点调整为用户确认的
  1 MiB / 8192 DOM 节点；既有元素/样式白名单、CSP、iframe sandbox 和深度限制保持不变。
- HTTP WIT 强制要求的 upstream exports 只作为 ABI 适配：不实现 DLL 请求业务字段解析，不允许修改
  upstream Body，并保持未修改原文；这不扩大用户确认的 downstream 业务范围。

不在范围：

- 不修改 Android Payment 源码、数据库落库、DLL Server、TLS、Listener、规则或 Proxy Host ABI。
- 不实现 upstream DLL 请求字段级解析、AES 加密、Socket Frame 或任何未知表兼容回退。
- 不发布、不推送、不创建 PR、不触发远程 CI，不自动修改用户当前运行中的 Workspace。

需求确认记录：

- 2026-09-03：用户要求基于 `/Users/codin/Code/jp_gmofg_payment` 生成可供 downstream 使用的 DLL
  encode/decode 包，并明确不能只解析顶层字段，必须完整解析。
- 2026-09-03：用户确认只处理 downstream 的 Encode、Decode、Display；覆盖 `0000`、`0001`、
  `0002` 和 `D48` 等错误响应；允许保留区按位置命名；Encode 自动计算长度；接受当前 App 的
  字符长度和逐字符 round-trip 规则。
- 2026-09-03：用户确认未知表 ID 明确失败，不静默跳过。
- 2026-09-03 15:42:49 +08:00：用户追加确认 Display 必须渲染为 table 样式；此前仅要求完整递归
  HTML 的验收表述失效，替换为带表头、表体和逐字段行的 HTML table，禁止退回 JSON `<pre>`。
- 2026-09-03 15:59:03 +08:00：Host 回放发现完整 Credit table Display 的原三列输出为 505068 bytes，
  超过 Proxy 128 KiB 上限；紧凑表格虽低于 128 KiB，但保守 DOM 节点计数为 4107，超过 4096。
  用户确认调整限制，采用 1 MiB HTML 与 8192 DOM 节点；不放宽主动内容、外部资源或脚本能力。
- 2026-09-03 16:01:47 +08:00：用户追加要求渲染时每张 table 使用不同颜色。实现采用按稳定表序号
  生成的不同配色，不引入随机数，且只使用安全白名单允许的 `background-color`、`border-color` 和
  `color`。

## 需求就绪检查

- 问题、目标和成功结果：PASS。
- 范围与不在范围：PASS。
- 输入、输出和状态变化：PASS；输入/输出均为 HTTP Body Unicode string，Shift-JIS 字节转换由 Proxy
  Body codec 负责，包不持久化状态。
- 具体示例：PASS；已有 Credit、UnionPay 和 D48 三类实际/历史报文。
- 可判断验收标准：PASS，见下节。
- 会改变实现方向的未确认事项：零。
- 进入实现时间：2026-09-03 15:21:33 +08:00。

## 当前已验证事实、推断与未知

当前已验证：

- Payment 使用 Shift-JIS JSON HTTP Body；Java 响应入口按 Credit/UnionPay 类型选择 Gson model。
- `JsonAdapters.TableDeserializeAdapter` 按 Java `String.length()` 校验 `Length`，其写出函数固定抛出
  `UnsupportedOperationException`。
- 表结构为：风险 `0=412`、卡公司 `1=37+32n`、商户组合 `2/3=90`、Batch `4=9`、终端 AP
  `5=39`、CA 组 `6=5+602n`、品牌 `7=67`、IC 公司组 `8` 嵌套 `0/9`、通信 KID `9=10`、
  银联 `A=57`；上述长度含适用 terminator。
- 完整 Credit 样本的七张外层表均满足声明长度，且可分解为 13 个卡公司记录、62 个卡号范围、
  32 个 CA Key、6 个品牌、10 个 IC 公司组、25 个风险记录和 10 个通信 KID。
- 当前 Host 的 HTTP package binding 同时拥有上下行流水线，WIT 强制导出上下行 Decode/Encode/Display；
  因此包必须提供 upstream ABI 适配，不能省略 export。
- 当前 Proxy Display 安全渲染在清洗前限制 HTML 字节数、清洗中限制 DOM 节点和深度；table 元素已在
  安全白名单，原限制会拒绝完整 DLL 参数表，不是 Component Decode/Encode 失败。

推断：保留区业务名称无法从当前 Java 源码恢复；按用户确认使用位置命名仍能无损解析和重编码。

未知：没有真实 GMO-FG Server 成功响应的本次在线抓包；本任务以当前 Android 源码、已有完整样本和
历史 D48 实际抓包建立可重复的包级合同，不将其描述为真实 Server 联机验收。

## 最小改动与最优设计

| 方案 | 分析 |
| --- | --- |
| 最小改动 | 复制 JSON Pretty 示例并仅拆少量字段，无法覆盖嵌套定长表、保留区和反向编码，会继续形成部分解析，拒绝采用。 |
| 最优设计 | 新建自包含 GMO-FG DLL HTTP Component；领域 model 拥有全部字段，parser/encoder 纯函数处理字符边界，WIT 层只映射错误，Display 独立渲染；fixture 与单元测试锁定 round-trip。 |

采用最优设计。不增加 Proxy 生产依赖、不修改 WIT 或 runtime ABI；包内仅使用仓库现有 `serde`、
`serde_json` 和 `wit-bindgen` 依赖模式。用户追加确认后，只调整前端不可信 Display 的资源上限和
对应提示，不放宽 HTML 安全能力。

## 验收标准

1. `0000`、Credit `0001`、UnionPay `0002`、D48 均能 decode 为稳定递归 Document。
2. Credit/UnionPay 所有已知表和保留区均进入 Document；没有阈值为空即丢弃整条风险记录的行为。
3. 未修改 Document 的 Encode 与原始 Body 逐字符相同。
4. 修改各类代表字段后，Encode 自动重建表和 `Length`，再次 Decode 得到修改后的值。
5. 非法外层长度、非法固定宽度、CA 数量不一致、未知表 ID、错误交易类型组合全部 fail-closed。
6. Display 使用带 `<thead>`、`<tbody>` 的 HTML `<table>`，完整展示公共响应字段、表名、嵌套记录
   和保留区，对 HTML 特殊字符转义，不包含 JSON `<pre>`，并能在 1 MiB / 8192 节点限制内通过
   Proxy 安全渲染；每张 table 的稳定配色不同；超过新限制仍 fail-closed。
7. 原生 Rust tests、fmt、strict Clippy、`wasm32-wasip2` release build、嵌入 Manifest 校验和 Host
   Component 加载/调用验证通过；无法执行的真实 Server/Android 联机项明确记录 `NOT_RUN`。

## 小任务、测试、文档与审查

| ID | 内容 | 依赖 | 状态 | 验收 |
| --- | --- | --- | --- | --- |
| DLL-01 | 建立自包含 Component、Manifest、fixture 与 RED 测试 | 无 | 已完成 | 测试覆盖四类响应和非法结构 |
| DLL-02 | 实现完整 downstream parser/domain Document | DLL-01 | 已完成 | 所有已知表逐字段解析 |
| DLL-03 | 实现自动长度 Encode、无修改保真和失败路径 | DLL-02 | 已完成 | round-trip 与修改后重解析 PASS |
| DLL-04 | 实现安全 table Display、调整已确认资源上限、构建脚本和文档 | DLL-02 | 已完成 | 完整 Credit 通过 1 MiB / 8192 节点门禁且 Component 可构建导入 |
| DLL-05 | 保存证据、整体对抗审查和日期归档 | DLL-01..04 | 已完成 | 高优先级审查无未解决 P0/P1 |

测试计划：包内单元测试使用活动 Credit、UnionPay、D48 和 Connection Test fixture；针对每种表执行
结构解析、未修改精确 Encode、代表字段变更、长度重算和失败变异；随后运行 Component 构建和 Host
WIT 调用验证。测试证据保存实际输入、Document、Encode 输出、比较结果、命令和环境。

文档影响：新增包 README；必要时同步协议包示例入口，不修改现有架构合同。

对抗审查计划：实现与验证完成后执行独立的任务级代码/协议审查，重点检查字符索引、保留区、
terminator、数量一致性、未知表失败、错误响应和 Encode 非对称问题；修复 P0/P1 后重新验证。

## 实施、测试与完成总结

- 新增 `examples/protocol-packages/gmofg_payment_dll/` 自包含 HTTP Wasm Component、完整 Manifest、
  构建脚本、四类 fixture、领域 Document、严格 parser/encoder、table Display 和单文件 `.wasm` 产物。
- downstream 支持 `0000/0001/0002` 与 D48；表 `0..9/A` 全部按固定字符位置解析，IC 嵌套表保持顺序，
  Java 未建模的三个保留区进入 Document；未知字段、未知表和结构错误 fail-closed。
- Encode 未修改时返回原始 Body；修改时重建各表并自动计算 `Length`。Credit 全部表代表字段和
  UnionPay `A` 表修改均经过再次 Decode 验证；增加 Card Range 后 `KCCI_01` 为 2510 字符。
- Display 使用对象字段列、数组记录行和嵌套分表；caption 显示路径，每张 table 使用不同且确定的
  安全配色，HTML 值全部转义。完整 Credit Host 输出 68424 bytes、保守 4107 DOM 节点。
- 用户确认后将 `ProtocolSafeDisplay` 上限从 128 KiB / 4096 节点调整为 1 MiB / 8192 节点；深度
  128、元素/样式白名单、主动内容删除、CSP 与无能力 iframe 均保持不变，超过新上限仍拒绝渲染。
- 测试：包级 13/13、Host Component 1/1、安全 Display 11/11；包和 Host strict Clippy、Rust format、
  Deno build check、TypeScript typecheck、目标 ESLint、release build、Manifest 校验、资源逐字节比较与
  `git diff --check` 全部 PASS。产物 547533 bytes，SHA-256
  `0af5550bffb871a2390d2ec2651c3a639aa06439c8b0c1057784b5ae87b51f13`。
- 测试证据：[`gmofg-dll-downstream-package`](../../../testing/evidence/2026-09-03/TASK-20260903-005/gmofg-dll-downstream-package/README.md)。
- 整体对抗审查：检查字段边界、terminator、CA 数量、保留区、空白阈值、交易表组合、错误路径、
  Encode 对称性、HTML 转义、资源上限和清洗边界；实现中发现并修复完整 Display 被旧资源门禁拒绝的
  P1，最终未解决 P0/P1/P2 为 0。
- CI、真实 GMO-FG Server 成功响应和 Android 设备端到端均 `NOT_RUN`；本地 fixture 与 Host PASS
  不替代这些外部验收。
- 修改文件：`examples/protocol-packages/gmofg_payment_dll/**`、
  `src-tauri/crates/package-runtime/tests/gmofg_payment_dll_component.rs`、
  `src/features/shared/protocol-safe-display.tsx`、
  `src/features/shared/protocol-safe-display.test.tsx`、任务与测试证据索引/档案。
