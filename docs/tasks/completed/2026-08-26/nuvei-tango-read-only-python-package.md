# Nuvei Tango 只读 Python 外部协议包

## 任务信息

- 任务 ID：`TASK-20260826-003`
- 状态：已完成
- 任务日期：2026-08-26
- 创建时间：2026-08-26 14:25:34 +08:00
- 开始时间：2026-08-26 14:25:34 +08:00
- 最后更新时间：2026-08-26 15:42:02 +08:00
- 完成时间：2026-08-26 15:42:02 +08:00
- 创建路径：`docs/tasks/pending/2026-08-26/nuvei-tango-read-only-python-package.md`
- 归档路径：`docs/tasks/completed/2026-08-26/nuvei-tango-read-only-python-package.md`
- 关联提交：`ca6cf66`（实现、测试与证据）；本次任务关闭提交（归档与索引）
- 关键词：Nuvei、Tango、Socket、Python、外部协议包、JSON、只读解析、字节保持
- 任务优先级：低。用户明确这是小任务；改动隔离在新增 Python 示例包内，只读、可逆且不改变
  Proxy 公共合同或生产源码。

## 背景与当前事实

### 当前已验证

- `tangodev.nuvei.com:9081` 可以完成公开 CA 验证的 TLS 1.3 握手，不要求客户端证书。
- Socket Listener 的 `relay + direct` 已把真实 `AccptrCmpltnAdvc` 和 `AccptrAuthstnReq` 报文发送至
  后台，并把对应响应发送回 App。
- 已观察线路帧为 4 字节大端 body 长度，后接 4 字节不透明控制头、8 字节 ASCII 数字序号和 UTF-8
  JSON 对象；长度值不包含自身 4 字节。
- 现有 `examples/external-packages/au_eftex` 证明 Python、WebSocket JSON-RPC API 1 和
  `/packages` 外部包链路可用。

### 推断与未知

- 4 字节控制头的内部位语义未知，本任务只作为不透明字节解析和展示。
- 8 字节数字字段的厂商正式名称未知，本任务使用中性名称 `sequence`，不赋予业务含义。
- 报告中的长帧展示被截断，不能作为完整自动化 fixture；测试使用同结构的合成、非支付数据。

## 目标

新增独立 Python 外部协议包 `nuvei-tango-json`：严格拆分线路帧，结构化展示非敏感元数据和 JSON，
并在 encode 阶段只允许未经修改的 Document 返回原始线路字节。任何字段或认证上下文变化均 fail-closed。

## 范围

- 新增 `examples/external-packages/nuvei_tango_json/` Python 包、测试和操作文档。
- 支持上下行 `split_frame`、`decrypt_message`、`encrypt_message`、`render_message`。
- 支持 TCP 拆包、粘包和单帧最大值限制。
- JSON 解析拒绝重复键、非有限数字、非对象顶层和尾随数据。
- 展示层递归掩码 PAN、Track2、密钥、MAC 等敏感键。
- 使用进程内随机 HMAC key 认证 encoding context，保证只读和原始字节恢复。

## 不在范围

- 不修改 Rust、Tauri、前端、数据库 Schema 或 Proxy 运行时。
- 不允许规则修改 JSON、控制头、序号或线路字节。
- 不推断控制头业务含义，不实现支付业务校验或 MAC 验证。
- 不归档、不提交或重放真实 PAN、Track2、密钥、MAC 和完整交易报文。
- 不改变 Listener 70 秒读取超时；该配置问题单独处理。

## 需求确认记录

- 2026-08-26：用户确认使用 Python 外部软件包。
- 2026-08-26：用户确认第一版为“只读解析”。
- 2026-08-26：用户明确按小任务处理，无须执行对抗审查。
- 2026-08-26：用户追加要求提供足以远程排查问题的日志。追加范围仅包含 Python 包结构化 stdout
  日志；必须包含连接/RPC 阶段、方向、字节数、结果、错误码和耗时，禁止记录报文、Base64、JSON
  内容、字段名或字段值。
- 2026-08-26：用户提供真实失败报告并要求检查。报告显示 Python decode 已执行，但 Proxy 在解析
  返回的 external Document wire 时产生 `Fatal(InvalidResponse)`；允许只修改 Python 包并真实复测。
- 只读定义：decode 和 display 可观察；encode 只在 Document 与原始快照完全一致时返回原始 frame；
  任何变化或 encoding context 篡改都返回错误。

## 需求就绪检查

- 问题、目标和成功结果：已明确。
- 范围与不在范围：已明确。
- 输入：长度前缀、控制头、序号、JSON；输出：只读 Document、掩码 HTML、原始字节。
- 错误行为：严格校验并 fail-closed，不透明降级。
- 示例：使用合成 `AccptrAuthstnReq`/`AccptrAuthstnRspn` 风格 JSON 帧。
- 验收标准：聚焦测试可直接判定拆帧、解析、掩码、篡改拒绝和逐字节保持。
- 会改变实现方向的未确认事项：无。
- 进入实现时间：2026-08-26 14:25:34 +08:00。

## 最小改动与最优设计

| 方案 | 结论 |
| --- | --- |
| 最小改动 | 复制现有包后替换 codec，重复 JSON-RPC/WebSocket 客户端较多，不采用整包复制。 |
| 最优设计 | 新建隔离 Python 包；保持 codec、RPC 和连接生命周期边界，使用标准库完成严格解析与认证，不增加依赖、不修改 Proxy。 |

## 小任务

| ID | 内容 | 依赖 | 状态 | 验收 |
| --- | --- | --- | --- | --- |
| NTV-001 | RED：帧、只读、RPC 合同测试 | 无 | 已完成 | 新测试因实现缺失失败 |
| NTV-002 | 实现 codec、RPC、client 和入口 | NTV-001 | 已完成 | 聚焦测试通过 |
| NTV-003 | README、证据和完整验证 | NTV-002 | 已完成 | 全套测试、compileall、静态检查通过 |
| NTV-004 | 安全结构化连接/RPC 日志 | NTV-002 | 已完成 | 正常与错误日志可诊断且无 payload 泄漏 |
| NTV-005 | 修复 int wire 并完成真实双向 Exchange | NTV-004 | 已完成 | 无 InvalidResponse；上下行逐字节保持 |

## 测试计划

- 完整帧、分段 `need_more`、粘包 `consumed_bytes`。
- 长度过小、超过上限、控制头缺失、非数字序号、非法 UTF-8、重复 JSON key、非对象 JSON。
- 上下行 decode/encode 逐字节一致。
- 修改任意 Document 字段、删除或篡改 encoding context 必须拒绝。
- Display 必须掩码敏感字段且不包含原值。
- JSON-RPC 注册、四个 hook、Base64 和错误映射。
- external Document `int.value` 必须是 i64 canonical decimal string，禁止返回 JSON number。
- 日志覆盖连接生命周期、RPC 方法/方向/阶段、输入输出字节数、拆帧状态、稳定错误码和耗时；序列化
  日志不得包含 Base64、完整报文、JSON 内容、字段名或字段值。
- 包全套单元测试、`compileall`、`git diff --check`；不执行真实授权报文重放。

## 对抗审查

N/A。用户明确这是低优先级小任务且无须对抗审查。边界输入、敏感字段掩码、只读篡改拒绝和
工作区范围仍由针对性自动化测试与最终 diff 检查覆盖。

## 实施记录

- 2026-08-26 14:25:34 +08:00：登记任务；生产实现尚未开始。
- 2026-08-26 14:27:47 +08:00：用户将任务定为低优先级小任务并取消对抗审查；实现范围不变。
- 2026-08-26 14:30:00 +08:00：TDD RED，测试因新包尚不存在产生预期 `ModuleNotFoundError`。
- 2026-08-26 14:34:00 +08:00：实现独立 codec、严格 JSON-RPC、WebSocket client、入口和包元数据；
  未修改 Proxy Rust/Tauri/前端。
- 2026-08-26 14:40:14 +08:00：12 个测试、compileall、diff check、wheel 构建全部通过。
- 2026-08-26 14:40:14 +08:00：用户报告中的一条完整 647 字节真实响应仅在内存验证；声明 body
  643 字节、消息类型 `AccptrCmpltnAdvcRspn`、Proxy 收发字节相同、package round-trip 字节相同、
  7 个敏感字段被掩码；未归档原报文、未联网重放。
- 2026-08-26 14:42:37 +08:00：实现与测试已完成；共享工作区存在其他 Agent 大量未提交修改且用户未
  要求提交，本任务保持 `进行中`，不在不安全的共享状态提交或移动 completed 归档。
- 2026-08-26 14:45:00 +08:00：用户追加诊断日志要求；TDD RED 因缺少 `rpc_started` 产生预期失败。
- 2026-08-26 14:50:00 +08:00：新增连接生命周期和 RPC 单行 JSON 日志；错误路径输出稳定
  `DECODE_FAILED`，只记录元数据。
- 2026-08-26 14:52:41 +08:00：13/13 包测试、compileall 和 diff check PASS；日志测试证明正常与
  错误路径可诊断，且不包含 Base64、合成敏感值或 Document 字段名。证据：
  `docs/testing/evidence/2026-08-26/TASK-20260826-003/NUVEI-PKG-002/`。
- 2026-08-26 15:21:59 +08:00：真实失败报告确认 `Fatal(InvalidResponse)` 发生在 Proxy 反序列化
  decode 返回值；根因是 Python 把 `frame_length` 的 `int.value` 返回为 JSON number，而 Proxy 合同
  要求 canonical decimal string。
- 2026-08-26 15:26:00 +08:00：TDD RED 复现 `245 != "245"`；最小修复仅把 Python wire 改为字符串，
  14/14 包测试、compileall 和 diff check PASS，未修改 Proxy 代码。
- 2026-08-26 15:36:32 +08:00：修复后的包连接 `ws://10.0.28.85:8765/packages`；三组真实双向
  Exchange 的 split/decode/display/encode 全部成功，首组 1602 B 上行和 647 B 下行均逐字节保持，
  未再出现 `InvalidResponse`、`DECODE_FAILED` 或包断连。证据：
  `docs/testing/evidence/2026-08-26/TASK-20260826-003/NUVEI-PKG-003/`。

## 修改文件、测试结果与完成总结

### 修改文件

- `examples/external-packages/nuvei_tango_json/README.md`
- `examples/external-packages/nuvei_tango_json/pyproject.toml`
- `examples/external-packages/nuvei_tango_json/nuvei_tango_json/*.py`
- `examples/external-packages/nuvei_tango_json/tests/*.py`
- `docs/testing/evidence/2026-08-26/TASK-20260826-003/NUVEI-PKG-001/`
- `docs/testing/evidence/2026-08-26/TASK-20260826-003/NUVEI-PKG-002/`
- `docs/testing/evidence/2026-08-26/TASK-20260826-003/NUVEI-PKG-003/`
- `docs/testing/evidence/README.md`
- `docs/tasks/README.md`
- 本任务文档

### 测试结果

- RED：2 个测试模块预期失败，原因是实现尚不存在。
- 初始只读包 GREEN：12/12 PASS，包括本机真实 WebSocket 注册。
- 追加日志后的结果：13/13 PASS。
- external Document wire 修复后的最终结果：14/14 PASS。
- Python `compileall`：PASS。
- `git diff --check`：PASS。
- Python wheel：PASS，`nuvei_tango_json-1.0.0-py3-none-any.whl`。
- 敏感/生成物检查：PASS；没有 13 至 19 位连续数字、真实交易 fixture、`.egg-info` 或 `.whl`
  残留在包目录。
- 证据：`docs/testing/evidence/2026-08-26/TASK-20260826-003/NUVEI-PKG-001/`。
- 日志证据：`docs/testing/evidence/2026-08-26/TASK-20260826-003/NUVEI-PKG-002/`。
- 真实双向 Exchange 证据：
  `docs/testing/evidence/2026-08-26/TASK-20260826-003/NUVEI-PKG-003/`。

### 当前总结

Python 外部包实现、RPC wire 修复、自动化和真实双向 Exchange 均已完成。实现、测试与证据已由
`ca6cf66` 提交；任务已移动 completed 归档并更新完成与测试索引。共享工作区的其他 Agent 修改未纳入
本任务提交。
