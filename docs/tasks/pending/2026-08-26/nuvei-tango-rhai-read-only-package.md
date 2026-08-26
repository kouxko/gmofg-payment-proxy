# Nuvei Tango 只读 Rhai 协议包

## 任务信息

- 任务 ID：`TASK-20260826-004`
- 状态：已阻塞
- 任务日期：2026-08-26
- 创建时间：2026-08-26 16:12:47 +08:00
- 开始时间：2026-08-26 16:18:08 +08:00
- 最后更新时间：2026-08-26 16:37:44 +08:00
- 完成时间：N/A
- 创建路径：`docs/tasks/pending/2026-08-26/nuvei-tango-rhai-read-only-package.md`
- 归档路径：`docs/tasks/completed/2026-08-26/nuvei-tango-rhai-read-only-package.md`
- 关键词：Nuvei、Tango、Socket、Rhai、协议包、ZIP、Python parity、只读解析
- 任务优先级：低。用户明确这是简单任务；实现必须隔离在新增 Rhai 包、构建脚本和包级测试内，
  不修改 Proxy Rust/Tauri/前端代码或公共合同。

## 背景与目标

派生自 `TASK-20260826-003` 的 Python 包。新增可直接导入 Intercept Proxy 的 Rhai ZIP 包，使用同一批
有效测试 Frame 分别运行 Python 与 Rhai，并以输入输出对照作为最终验收。

## 范围

- 新增独立 Rhai 包源码、Document Schema、确定性 ZIP 构建脚本和包级测试。
- Frame：4 字节大端 body length + 4 字节控制头 + 8 字节 ASCII sequence + JSON object。
- Document 保持六字段：`frame_length`、`control_header`、`sequence`、`message_type`、
  `json_preview`、`encoding_context`。
- `json_preview` 和 Display 原样保留 JSON 字段名与字段值，不执行任何脱敏、掩码或替换。
- Frame、字段类型、只读 Encode 和 fail-closed 行为与 Python 版一致。
- 最终交付 ZIP、SHA-256、导入说明和复测命令。

## 不在范围

- 不修改 Proxy 源码、数据库、Host API 或现有 Python 包。
- 不处理非法 UTF-8。
- 不处理重复 JSON key。
- 不允许业务字段修改，不重编码线路 JSON，不记录真实敏感报文。
- 不实现敏感字段识别或脱敏规则。

## 需求确认记录

- 2026-08-26：用户要求使用 Rhai 生成一版可导入 ZIP。
- 2026-08-26：用户明确不考虑非法 UTF-8和重复 JSON key。
- 2026-08-26：用户确认最终验收只看 Rhai 与 Python 输入输出一致。
- 2026-08-26：用户要求先规划，本次不实现、不生成 ZIP。
- 2026-08-26 16:14:43 +08:00：用户明确 Rhai 版不脱敏。该变更替代原“展示与 Python 掩码结果一致”
  的验收项；Python 继续作为 Frame、字段结构、错误和 Encode 的 oracle，JSON 展示改为原文验收。

## 需求就绪检查

- 目标、范围和交付物：已明确。
- 输入与输出：Frame、字段结构、错误和 Encode 以 `TASK-20260826-003` Python codec 为 oracle；
  `json_preview`/Display 按原始 JSON 内容验收。
- 错误行为：除两个明确排除项外，同一输入必须与 Python 同成功或同失败；失败时 0 线路输出。
- 具体示例：复用合成 `AccptrAuthstnReq`/`AccptrAuthstnRspn` Frame，并重做已验证的
  1602/647、1602/914、1322/896 B 双向 Exchange。
- 验收：逐字段、逐语义、逐字节对照可直接判定 PASS/FAIL。
- 未确认事项：无。

## 最小改动与最优设计

| 方案 | 结论 |
| --- | --- |
| 最小改动 | 新增隔离 Rhai 包与 Python 标准库 ZIP builder，不改 Proxy；采用。 |
| 最优设计 | Decode 只生成展示 Document；Encode 从可信 `origin` 重算预期 Document，完全一致才返回 origin。 |

## 实施计划

| ID | 内容 | 验收 |
| --- | --- | --- |
| NTR-001 | 建立 Python parity fixtures/oracle | 已完成；同一输入生成 Python expected 和 Rhai actual 对照 |
| NTR-002 | 实现 Frame、Decode、原文 JSON Display | 已完成；JSON 字段名和值全部保留，无 `[redacted]` 或其他替换 |
| NTR-003 | 实现只读 Encode | 已完成；未修改时 byte-exact，六字段修改/删除均 fail-closed |
| NTR-004 | 构建确定性 ZIP | 已完成；连续两次 SHA-256 相同，ZIP 通过真实 Host 导入编译执行 |
| NTR-005 | 真实双向 Exchange 与归档 | 已阻塞；当前没有 Proxy 写入控制面或授权测试 App，真实用例 NOT_RUN |

## 测试计划与最终验收

1. Python 与 Rhai 接收完全相同的合成 Frame。
2. 对比 Frame decision 和六个 Document 字段及类型；`json_preview`/Display 必须保留原始 JSON 语义和所有值。
3. 未修改 Encode 输出必须与输入 Frame 逐字节相同。
4. 六字段逐一修改、删除、context 篡改和跨方向复用必须与 Python 一样失败，线路输出 0 B。
5. 覆盖 TCP 分段、粘包、长度边界、非法 sequence、JSON 语法/顶层结构；非法 UTF-8与重复 key 标记 N/A。
6. ZIP 导入、启用、Listener 双向运行以及 1602/647、1602/914、1322/896 B 实际 Exchange 全部 PASS。
7. 除已确认的“Rhai 展示不脱敏”差异外，任一非排除项输入输出与 Python 不一致，任务不验收、不交付 ZIP。

## 文档、证据和提交

- 证据目录：`docs/testing/evidence/2026-08-26/TASK-20260826-004/`。
- 保存源码、合成非支付输入、Python expected、Rhai actual、comparison、ZIP listing/SHA-256、导入步骤和安全日志；
  真实链路只记录阶段、方向、字节数和结果，不保存原文 JSON。
- 完成后更新包 README、任务文档、任务索引和测试证据索引，并独立提交；不包含其他 Agent 修改。

## 当前实施记录

- 2026-08-26 16:12:47 +08:00：完成规划并登记任务；尚未实现，尚未生成 ZIP。
- 2026-08-26 16:14:43 +08:00：需求变更为不脱敏；已更新范围与验收，仍未开始实现。
- 2026-08-26 16:18:08 +08:00：重新确认需求就绪、Python oracle、Rhai Host 合同及共享工作区边界；
  任务进入实现，仅修改新增 Rhai 包、包级测试、构建产物和本任务档案。
- 2026-08-26 16:33:09 +08:00：完成独立 Rhai 包、Document Schema、只读 Encode、原文 Display、
  Python oracle、6 个包级测试、确定性 ZIP builder、导入说明和最终制品。
- 2026-08-26 16:33:09 +08:00：`cargo fmt --check`、Clippy `-D warnings`、6/6 测试、Python
  `compileall`、两次确定性构建、ZIP listing/SHA-256 和 diff check 全部 PASS；包级证据为
  `docs/testing/evidence/2026-08-26/TASK-20260826-004/NTR-RHAI-001/`。
- 2026-08-26 16:37:44 +08:00：真实链路检查确认 `10.0.28.85:8765` 只提供 external-package
  WebSocket，当前主机没有本地 Listener 或 ADB 设备，MCP 仍为只读，无法导入/启用 Rhai 包并由授权
  App 发起交易。`NTR-RHAI-002` 保持 NOT_RUN，任务转为已阻塞，不归档为完成。

## 实施结果

### 已完成

- 新增 `nuvei-tango-json-rhai@1.0.0`，上下行均声明 Frame、Decode、Encode 和 Display。
- 严格实现 4 B 大端长度、4 B 控制头、8 B 数字 sequence、单顶层消息对象和 1 MiB Frame 上限。
- Document 固定六字段；`json_preview` 与 Display 保留原始合成 JSON 字段名和值，HTML 特殊字符转义。
- Encode 从可信 `origin` 重新 Decode 并逐字段比较；六字段任一修改或删除、context 篡改及跨方向复用
  均失败，不返回线路字节。
- 最终 ZIP：
  `examples/protocol-packages/nuvei_tango_rhai/dist/nuvei-tango-json-rhai-1.0.0.zip`。
- SHA-256：`0595af171e20ae9eee21da42a8327971c99689a278cab6ffd7612ba20a4049ea`。

### 未完成与阻塞

- 未在 `10.0.28.85` 的真实 Proxy UI 导入或启用 Rhai ZIP。
- 未绑定真实 Nuvei Listener，未由授权测试 App 产生三组当前 Exchange。
- `TASK-20260826-003 / NUVEI-PKG-003` 的历史真实交易只作为派生背景，不作为本任务 PASS。
- 解除条件：取得真实 Proxy UI 或等价已授权写入控制面，并连接授权测试 App；按
  `NTR-RHAI-002/steps/replay.md` 执行后才能完成验收与归档。

## 测试与验收结果

| 层级 | 结果 | 证据 |
| --- | --- | --- |
| Rhai 源码/Schema/Manifest 编译 | PASS | `NTR-RHAI-001` |
| Python 同帧 parity | PASS | `python-expected.json`、`rhai-actual.json`、`comparison.json` |
| Frame/Decode/Display/Encode | PASS | 6/6 包级 Host runtime 测试 |
| 六字段修改/删除/context/跨方向失败路径 | PASS | `NTR-RHAI-001` |
| 1602/647、1602/914、1322/896 B 合成双向运行 | PASS | `NTR-RHAI-001` |
| ZIP 确定性、安全读取、导入编译和执行 | PASS | 两次同 SHA-256；`NTR-RHAI-001` |
| 真实 Proxy 导入、启用、Listener 与交易 | NOT_RUN | `NTR-RHAI-002` |
| CI | N/A | 未获授权触发远程 CI |

## 修改文件与附加文件

- `examples/protocol-packages/nuvei_tango_rhai/README.md`
- `examples/protocol-packages/nuvei_tango_rhai/{manifest.toml,document.toml,protocol.rhai,display.rhai}`
- `examples/protocol-packages/nuvei_tango_rhai/build_package.py`
- `examples/protocol-packages/nuvei_tango_rhai/dist/*`
- `examples/protocol-packages/nuvei_tango_rhai/tests/**`
- `docs/testing/evidence/2026-08-26/TASK-20260826-004/NTR-RHAI-001/`
- `docs/testing/evidence/2026-08-26/TASK-20260826-004/NTR-RHAI-002/`
- `docs/testing/evidence/README.md`
- 本任务文档。

## 文档影响分析与同步

| 文档 | 结论 |
| --- | --- |
| 包 README | 需要更新；已新增范围、线路合同、构建、导入、启用、复测和限制 |
| `docs/README.md` | 无需更新内容；pending 入口保持有效，任务尚未完成 |
| `docs/tasks/README.md` | 无需更新；任务未完成，不得加入完成索引 |
| `docs/testing/evidence/README.md` | 需要更新；已登记 PASS 包级证据和 NOT_RUN 真实链路证据 |
| 架构、需求、MCP、前端和发布文档 | 无需更新；未修改公共合同或产品代码 |

## 对抗审查决定

未执行独立对抗审查。任务为隔离的低优先级示例包，不修改公共合同或产品源码；风险由真实 Host runtime
编译执行、Python parity、逐字段 fail-closed、Clippy `-D warnings`、确定性 ZIP 和证据一致性检查覆盖。
任务当前也未进入完成归档门禁。
