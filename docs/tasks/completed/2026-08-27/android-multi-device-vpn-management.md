# Android 多设备 VPN 并行运行与逐设备管理

## 任务信息

- 任务 ID：`TASK-20260827-001`
- 状态：`已完成`
- 任务日期：`2026-08-27`
- 创建时间：`2026-08-27 11:10:28 +08:00`
- 开始时间：`2026-08-27 12:20:45 +08:00`
- 最后更新时间：`2026-08-27 14:49:30 +08:00`
- 完成时间：`2026-08-27 14:49:30 +08:00`
- 创建路径：`docs/tasks/pending/2026-08-27/android-multi-device-vpn-management.md`
- 归档路径：`docs/tasks/completed/2026-08-27/android-multi-device-vpn-management.md`
- 关键词：`Android`、`VPN`、`multi-device`、`runtime owner`、`ADB Reverse`、`LAN`、`disconnect`、`reconnect`
- 任务优先级：`高`
- 优先级理由：改变持久化 Schema、公共命令合同、并发/生命周期所有权、ADB 资源清理和跨设备隔离；失败可能停止或覆盖错误设备的运行态。

## 背景与实际现象

当前桌面端只保存一个 `runtime_owner`。设备 A 运行 VPN 后，即使选择在线设备 B，前端和 Rust
后端都会阻止 B 启动；A 拔出后记录进入 `waiting_reconnect`，停止/紧急恢复因 ADB 不可达而无法
释放单一所有权。

用户最初要求允许新设备接管，进一步确认后的真实目标是：多个设备同时运行 VPN，并可以分别管理，
因此本任务不是单一 owner 的“强制释放”补丁，而是多运行所有者模型。

### 发生环境与提供资源

- 用户截图：旧运行设备 `1850872507` 已断开，新选择设备 `18504501104`，页面提示必须先停止旧设备。
- 当前源码与现有 owner/frontend 测试。
- `.omx/context/android-multi-device-vpn-management-20260827T031028Z.md`。
- `.omx/specs/deep-interview-android-multi-device-vpn-management.md`。

## 目标

支持最多 8 台 Android 设备使用独立方案同时运行 VPN，并让安装、更新、授权、应用清单、启动、
应用、状态、停止、紧急恢复和运行端点查询全部显式绑定到目标设备。任何设备断线、失败或重连不得
阻止、覆盖、停止或清理其他设备。

## 范围

- 将单一运行所有者升级为按设备序列号索引、epoch 保护、上限 8 条的运行记录集合。
- ADB Reverse、LAN、仅设备端三种模式均支持多设备并行。
- 每台设备使用独立方案；允许不同设备引用同一个已保存方案，但运行事实、epoch 和端点仍独立。
- 完整逐设备管理：组件安装/更新、VPN 授权、应用清单、启动/应用、状态、停止、紧急恢复、端点查看。
- 设备断线后保留记录并等待同序列号重连，不影响其他设备。
- 桌面重启恢复全部运行/不确定/待清理/停止失败/断线记录。
- 新 Schema 直接采用多 owner 集合；不实现旧单 owner 数据迁移或兼容升级。
- 更新绑定、架构文档、操作说明和可复用测试证据。

## 不在范围

- 共享方案广播、批量启动、批量应用或批量停止。
- 超过 8 条并发运行记录或远程设备集群管理。
- 自动删除断线记录或把断线伪报为已停止。
- Android 页面之外的 UI 重构。
- 新依赖、Listener/协议包行为调整、发布或部署。
- 旧开发数据库到新 Schema 的数据保留、原位迁移或兼容升级。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-08-27` | 用户确认目标不是单设备接管，而是支持多个设备同时运行 VPN 并管理。 |
| `2026-08-27` | 每台设备使用独立方案。 |
| `2026-08-27` | 第一版提供完整逐设备管理。 |
| `2026-08-27` | 同时运行/保留的设备上限为 8 台。 |
| `2026-08-27` | 运行设备断线后保留记录并等待同序列号重连。 |
| `2026-08-27` | 用户明确“不考虑升级”：不做旧单 owner 数据迁移，沿用项目当前开发期旧库重建机制。 |

## 未确认事项

- 无会改变实现方向的产品事项。内部 Schema、命令 DTO、UI 表格/卡片形态和轮询调度由共识计划在既定边界内决定。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`
- 范围与不在范围：`PASS`
- 输入、输出与状态变化：`PASS`
- 错误行为：`PASS`，单设备错误必须隔离且保持真实状态，不允许默认成功。
- 具体示例：`PASS`，A 运行/断线期间 B 能独立启动，A 重连只恢复 A。
- 可重复 PASS/FAIL 验收：`PASS`
- 改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-08-27 11:45:45 +08:00（RALPLAN 共识门禁通过；生产源码尚未开始）`

## 问题与根因分析

### 当前已验证

- 前端根据单一 `runtimeOwnerSerial` 禁用其他设备的启动/应用。
- Rust `ensure_selected_can_activate` 拒绝与单 owner 不同的序列号。
- stop/status/emergency restore 只解析单一 owner；断线时保留它。
- SQLite、运行端点、Reverse ownership 和前端查询模型均以单一 owner 为中心。
- 相关前端 37 个测试和 Rust owner lifecycle 8 个测试通过，证明这是当前明确合同，不是单纯 UI 陈旧。

### 推断

- 只删除前端禁用条件会被后端拒绝，且即使绕过后端也会造成 owner/端点/清理事实互相覆盖。
- 仅把 owner 改成数组但保留隐式 `selected_serial` 命令会引入跨设备误操作与迟到响应覆盖。

### 未知

- 真实 2 至 8 台设备运行验收环境当前是否可用；自动化完成后按实际环境标记 `PASS` 或 `NOT_RUN`。

### 已确认根因

系统在 R02 中有意实现了“全局单 runtime owner”合同，并把选择设备与该 owner 分离；该合同解决了
错误设备停止问题，但无法表达多个合法并行 owner。UI 阻断只是表现，根因是 Application port、
Infrastructure 状态、SQLite 持久化、Tauri DTO 和前端查询均采用单值模型。

### 影响范围

- Application Android port/facade/requirements tests。
- Infrastructure ADB adapter、owner/reverse/runtime/endpoints、fresh SQLite Schema 和旧库重建路径。
- Host/Tauri commands、生成绑定和事件实体键。
- Android Network UI、查询 cache、轮询与组件测试。
- 架构、持久化、用户操作和测试证据文档。

## 最小改动与最优设计比较

| 方案 | 分析 |
| --- | --- |
| 最小改动 | 移除前端和 `ensure_selected_can_activate` 阻断，继续覆盖单一 owner。修改文件少，但会丢失先前设备的停止/清理/端点事实并产生跨设备误操作，违反验收，淘汰。 |
| 局部集合 | Infrastructure 内把 owner 改为集合，但保留隐式 selected-device Application/IPC。可减少 DTO 改动，但完整逐设备管理仍有 TOCTOU 和迟到响应污染，淘汰。 |
| 最优设计 | 建立上限 8 条的按 serial+epoch 多 owner 注册表；所有设备操作显式 serial，运行变更使用 epoch/CAS；持久化、事件、查询和 UI 全链使用同一身份。改动较广，但职责和错误模型正确，无双路径。 |

采用并冻结：`最优设计`。详见 `.omx/plans/prd-android-multi-device-vpn-management.md`；Architect Review 2 与 Critic Review 3 均为 `APPROVE`。

## 小任务列表

| ID | 任务 | 依赖 | 可并行 | 状态 | 验收 |
| --- | --- | --- | --- | --- | --- |
| MDV-01 | 共识架构、ADR 与测试规格 | 无 | 否 | 已完成 | Architect Review 2 与 Critic Review 3 均 `APPROVE` |
| MDV-02 | 多 owner 领域/Application/IPC 合同与绑定 | MDV-01 | 否 | 已完成 | 显式 serial/epoch 合同测试通过 |
| MDV-03 | SQLite 多 owner Schema 与原子注册表 | MDV-02 | 否 | 已完成 | 全新 Schema、8/9 容量、原子回滚、重启测试通过 |
| MDV-04 | ADB Reverse/LAN/device-only 多设备生命周期 | MDV-02, MDV-03 | 否 | 已完成 | 跨设备启动/停止/断线/重连/陈旧 epoch 隔离通过 |
| MDV-05 | 完整逐设备 UI、事件、轮询与异步隔离 | MDV-02 | 可与 MDV-04 在合同冻结后并行 | 已完成 | 每设备操作和迟到响应测试通过 |
| MDV-06 | 文档、证据、整体验证与对抗审查 | MDV-03, MDV-04, MDV-05 | 否 | 已完成 | 自动化与整体门禁通过；真机用例明确 NOT_RUN |

共享合同冻结前不得并行 MDV-03/04/05。并行时分别拥有后端运行时与前端文件，不得修改同一绑定文件。

## 测试计划

- 回归先行：把现有“B 被 owner A 阻止”测试改为新 RED 契约前，先保留并明确旧行为基线。
- Rust 单元/集成：A/B/C 混合模式、独立方案、8/9 容量、serial/epoch CAS、停止/恢复/端点隔离。
- SQLite：全新多记录 Schema、完整集合重启恢复、坏记录读取失败、并发替换、部分失败零丢失；旧库按现有重建机制处理。
- Application/Host/IPC：所有完整管理命令显式 serial；缺失/离线/陈旧 epoch 稳定错误码。
- 前端：最多 8 台卡片/表格、断线展示、每设备 action、cache/event identity、迟到响应隔离、可访问性。
- 兼容：Android Companion 协议不因桌面多 owner 误改；必要时只增加桌面命令参数。
- 静态/整体：bindings 幂等、typecheck、lint、architecture/source-size、fmt/clippy、受影响 crate、workspace 风险门禁。
- 真机：至少两台设备并行；资源不足时标记 `NOT_RUN` 并保留重放步骤。

## 对抗审查计划

- 计划阶段：Planner → Architect → Critic，顺序执行，Critic 非 `APPROVE` 则闭环修订。
- 实现阶段：独立 reviewer 检查跨设备误操作、陈旧 epoch 清除、fresh Schema/旧库重建边界、端口串扰、迟到 UI 响应、容量绕过。
- 完成前整体审查必须为 `APPROVE/CLEAR`；发现项修复后重新运行受影响验证。

## 文档影响

- `docs/architecture/android-vpn-transparent-routing.md`
- `docs/architecture/security-and-persistence.md`
- `docs/user-operation-guide.md`
- 必要的 ADR/模块/数据流追踪文档
- 本任务文档、测试证据 README/metadata 和索引

## 实施记录

- `2026-08-27 11:10:28 +08:00`：完成四轮需求访谈，旧的单设备显式接管方向被多设备并行目标替代。
- `2026-08-27 11:10:28 +08:00`：登记任务，进入 `$ralplan --deliberate` 共识计划阶段。
- `2026-08-27 11:20:22 +08:00`：用户明确不考虑升级，移除旧单 owner 原位迁移与数据保留要求；全新 Schema 沿用开发期旧库重建合同。
- `2026-08-27 11:40:12 +08:00`：Architect 初审要求补充 Application gate、全 owner shutdown、Environment set-diff 和数据库容量 trigger；修订后复核 `APPROVE`。Critic 初审要求冻结隐式写查询 gate、status epoch wire、精确 MCP/Environment 合同和锁顺序，已进入修订复审。
- `2026-08-27 11:45:45 +08:00`：Critic 第三次复审 `APPROVE`，无剩余 blocker；MDV-01 共识门禁完成，生产源码留给独立执行阶段。
- `2026-08-27 12:20:45 +08:00`：进入实现；先以多 owner 集合、显式 serial/epoch、8/9 容量和跨设备隔离 RED 锁定公共合同，再依次迁移持久化、运行时与 UI。
- `2026-08-27 14:49:30 +08:00`：完成 SQLite 多 owner、逐 serial/epoch 运行时、显式命令、Environment 集合 baseline、Host shutdown 和前端多设备管理；复审发现的错误归属、离线判定及迟到响应问题均已修复并回归。

## 修改文件

- Application Android ports/facades/models、Infrastructure ADB adapter/SQLite/Environment lease、Host/Tauri commands 与组合根。
- `src/features/android-network/**`、生成绑定、Android 架构/操作文档与对应测试。

## 附加文件

- `.omx/context/android-multi-device-vpn-management-20260827T031028Z.md`
- `.omx/interviews/android-multi-device-vpn-management-20260827T031028Z.md`
- `.omx/specs/deep-interview-android-multi-device-vpn-management.md`
- `.omx/plans/prd-android-multi-device-vpn-management.md`
- `.omx/plans/test-spec-android-multi-device-vpn-management.md`
- `.omx/plans/android-multi-device-vpn-management-architect-review-1.md`
- `.omx/plans/android-multi-device-vpn-management-architect-review-2.md`
- `.omx/plans/android-multi-device-vpn-management-critic-review-1.md`
- `.omx/plans/android-multi-device-vpn-management-critic-review-2.md`
- `.omx/plans/android-multi-device-vpn-management-critic-review-3.md`
- `.omx/plans/android-multi-device-vpn-management-consensus-handoff.md`

## 验收结果

- `PASS`。最多 8 条 owner、显式 serial/epoch、跨设备并行/隔离、断线保留、重连、逐设备停止/恢复和错误上下文均已实现。
- 真机 A/B 并行场景为 `NOT_RUN`：本机没有连接 Android 设备；不使用自动化 PASS 替代真机结论。

## 测试结果

- Infrastructure Android adapter 聚焦 `43/43 PASS`，包含 8/9 容量、A/B gate、stale epoch、forward/reverse 隔离与重连错误归属。
- Application Android 聚焦 `13/13 PASS`；Environment owner/lease/gate `9/9 + 25/25 + 7/7 + 8/8 PASS`。
- 前端全量 `67` 个文件、`659` 项 PASS；其中 Android 多设备、迟到响应、owner 卡片与 epoch 合同均通过。
- Rust workspace 全量、严格 Clippy、格式、架构、源码规模和 Windows 静态编译检查 PASS。

## CI 情况

- 用户已明确授权在全部任务完成后统一交付并触发远程 Windows 验证；结果由最终交付任务统一记录。

## 完成总结

- 多设备运行所有权已从单值模型完整升级为按 serial+epoch 的上限 8 条集合；selected device 不再作为执行回退，单设备失败不会覆盖或阻止其他设备。当前自动化与本地门禁全部通过，真机资源缺失项保持 `NOT_RUN`。
