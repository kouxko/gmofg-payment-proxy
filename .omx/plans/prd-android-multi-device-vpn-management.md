# PRD: Android 多设备 VPN 并行运行与逐设备管理

状态：`APPROVED_FOR_IMPLEMENTATION`（Architect APPROVE；Critic Review 3 APPROVE）

关联任务：`TASK-20260827-001`

## 结果

把 Android VPN 的全局单一运行所有者改为按设备序列号隔离、最多 8 条的运行注册表。每台设备拥有独立方案、epoch、端点、ADB Reverse 事实和生命周期；设备断开后仅自身进入 `waiting_reconnect`，同序列号重连时仅恢复自身。安装、更新、授权、应用清单、启动、应用、状态、停止、紧急恢复和端点查询全部显式携带目标 serial。

## 已冻结产品边界

- 每台设备独立选择方案；多个设备可以引用同一已保存方案，但运行事实不合并。
- 同时存在的运行记录最多 8 条；`waiting_reconnect`、`uncertain`、`cleanup_required` 和 `stop_failed` 都占用容量。
- 断线不等于停止，不自动删除记录，也不释放该设备的清理事实。
- 不提供批量启动、批量应用、批量停止、共享广播或远程设备集群能力。
- 不考虑升级：不迁移旧单 owner 数据，不保留旧数据库数据，沿用当前开发期版本不匹配即重建数据库的合同。
- 不增加依赖，不改变 Android Companion 协议，不修改 Listener/协议包行为。

## RALPLAN-DR

### 原则

1. `serial` 是设备操作身份，UI 当前选择只表示编辑上下文，不能成为运行时权限。
2. 每个运行变更都受 `serial + epoch` 保护；迟到操作不能清除同设备的新 epoch，更不能触碰其他设备。
3. 容量必须在任何设备端副作用之前原子预留；第 9 条记录失败时 8 条既有记录和设备状态均不变。
4. 同一 serial 的生命周期操作串行，不同 serial 可以并行；不得持全局锁跨越 ADB 或 SQLite `await`。
5. 断线、失败、不确定和待清理都是真实状态；不以默认成功、自动释放或跨设备清理掩盖失败。

### 主要决策驱动

1. 防止停止、清理、端点或迟到响应串到错误设备。
2. 在 ADB Reverse、LAN、仅设备端三种模式下都能表达 2 至 8 个合法并行 owner。
3. 桌面重启后恢复全部清理事实，并对容量、竞争和失败提供可重复验证。

### 方案 A：按 serial 的注册表、逐设备 gate、SQLite 行级 CAS

- 内存使用按 serial 索引的有界注册表；每个 serial 有独立生命周期 gate。
- SQLite 每个 serial 一行，epoch 唯一；保存、替换和删除使用 `serial + expected_epoch`。
- 所有 Application/IPC 命令显式 serial；运行变更携带 expected epoch。
- UI 由在线设备与运行记录的并集生成逐设备视图。

优点：身份、持久化、并发和 UI 查询使用同一键；跨设备并行且错误边界清晰；不需要新依赖。

缺点：Application、Infrastructure、Host、绑定、UI、诊断和环境配置消费者都需同步修改。

### 方案 B：每台设备一个常驻 actor，由全局 supervisor 管理

- 每个 serial 启动 actor，所有命令通过 channel 串行发送。
- supervisor 管理容量、重连和 actor 生命周期。

优点：同设备串行和取消边界天然明确，适合更大的设备集群。

缺点：当前最多 8 台且命令式 ADB adapter 已有清晰边界；actor 会引入 mailbox、actor 恢复、supervisor 崩溃和 channel 错误模型，显著扩大实现与测试面。

### 方案 C：单个 JSON 快照保存全部 owner，继续使用全局操作锁

- 一次读写完整集合，少量修改现有 SQLite API。

优点：Schema 和 CRUD 较少。

缺点：跨设备操作仍被全局串行；任何更新重写全部记录；难以表达 serial+epoch CAS、容量竞争和单记录故障隔离。

### 决策

采用方案 A。方案 B 对 8 台上限过度设计；方案 C 不能满足跨设备并行和行级所有权保护。

## 公共合同

### 设备目标

```rust
struct AndroidDeviceTarget {
    serial: String,
}

struct AndroidRuntimeTarget {
    serial: String,
    expected_epoch: Uuid,
}

struct AndroidNetworkStatusViewModel {
    serial: String,
    runtime_epoch: Option<Uuid>,
    // existing status fields remain
}
```

- 安装、更新、授权、应用清单/查询/刷新使用 `AndroidDeviceTarget`。
- `start(serial, activation)` 只在该 serial 没有运行记录时创建新 epoch；已有记录返回稳定的 `ANDROID_RUNTIME_ALREADY_MANAGED`。
- `apply(AndroidRuntimeTarget, activation)` 必须匹配当前 epoch，成功后产生新 epoch，使旧请求立即失效。
- `stop` 与 `emergency_restore` 必须匹配 serial+epoch；成功后只删除该行。
- `status(serial)` 与 `endpoints(serial)` 是显式 serial 查询，返回该设备事实；不得回退到 selected serial 或任意 owner。
- `runtime_owners()` 返回按 serial 稳定排序的集合，不再返回 `Option<Owner>`。
- epoch 不匹配返回稳定的 `ANDROID_RUNTIME_EPOCH_STALE`，记录不存在返回 `ANDROID_RUNTIME_NOT_MANAGED`，容量满返回 `ANDROID_RUNTIME_CAPACITY_EXCEEDED`。

`AndroidNetworkStatusViewModel.runtime_epoch` 是唯一成功状态/事件 epoch wire：

- Start 成功返回该次预留 epoch；Apply 成功返回新 epoch。
- Status 对已管理 serial 返回当前 owner epoch，包括 waiting/faulted；未管理 serial 的停止态查询返回 `None`。
- Stop/Emergency 成功删除 owner 后仍在终止响应/事件中返回请求的 expected epoch，使前端仅在当前 epoch 相等时清除；失败不返回该 DTO，按下方 AppError 合同关联权威 epoch。
- 安装、更新、授权和纯 package 操作不伪造 runtime epoch。
- 所有 Android VPN status 事件 payload 都包含该字段；`entityId=serial`。没有 epoch 的旧 payload 不被兼容接受。

失败合同单独使用现有 `AppErrorViewModel`，不能返回 success-shaped status：

- Start/Apply/Stop/Emergency/status/endpoints 任一失败始终为 `Err(AppError)`；Tauri/MCP 保持 error/rejection wire，不把失败序列化为 `AndroidNetworkStatusViewModel`。
- 若失败后 SQLite 中存在权威 owner/cleanup 记录，错误携带 `entity_id=serial` 与 `runtime_epoch=该持久化记录 epoch`。
- admission 前失败、not-managed 或其他没有权威 owner 的错误携带 `entity_id=serial`、`runtime_epoch=None`。
- 持久化新状态失败时不得附带未落库 epoch；若旧记录仍是权威事实，只能关联旧记录 epoch。不得发布以未落库 epoch 为权威的 status 事件。

所有 Host/Tauri DTO、生成 TypeScript 绑定、事件和 query keys 使用同一命名。旧的无 serial 命令签名直接删除，不保留双路径。

## 持久化设计

当前 Schema 直接创建复数语义表：

```sql
CREATE TABLE android_runtime_owners (
  serial TEXT PRIMARY KEY,
  epoch TEXT NOT NULL UNIQUE,
  mode TEXT NOT NULL,
  profile_id TEXT NOT NULL,
  state TEXT NOT NULL,
  source TEXT NOT NULL,
  transition_reason TEXT NOT NULL,
  reverse_ports_json TEXT NOT NULL,
  resume_state TEXT NULL,
  runtime_endpoints_json TEXT NOT NULL DEFAULT '[]',
  updated_at TEXT NOT NULL
);
```

现有 mode/state/source/reason CHECK 约束保留。旧 `android_runtime_owner(singleton_id=1)` 不迁移；Schema 版本提升后由现有 `ensure_current_schema` 重建旧开发数据库。

### 原子操作

- `load_android_runtime_owners()` 一次读取并逐行严格解析，按 serial 返回；任一损坏记录使整个权威快照读取失败，不跳过坏行。
- `reserve_android_runtime_owner(record)` 在 SQLite 事务中先判断 serial 是否已存在，再判断总数 `< 8`，最后插入准备态记录。容量检查与插入属于同一写事务。
- `reserve_android_runtime_owner` 使用 `TransactionBehavior::Immediate`，避免两个 admission 同时基于旧 count 决策。
- 数据库创建 `BEFORE INSERT` 容量 trigger：仅当 `NEW.serial` 尚不存在且当前行数 `>= 8` 时 `RAISE(ABORT, 'ANDROID_RUNTIME_CAPACITY_EXCEEDED')`。store 把该确定消息映射为稳定应用错误；已有 serial 的 upsert 不被 trigger 误拒绝。
- `replace_android_runtime_owner_if_epoch(serial, expected_epoch, record)` 只更新同 serial+epoch，且新 record.serial 必须相同。
- `clear_android_runtime_owner(serial, expected_epoch)` 只删除精确行。
- reset/workspace 清理语义改为删除复数表全部记录；不按 selected serial 隐式清理。

## 内存与并发所有权

```text
AndroidAdbAdapter
  owner_registry: RwLock<BTreeMap<Serial, DeviceRuntimeFacts>>
  operation_gates: Mutex<BTreeMap<Serial, Weak<Mutex<()>>>>
```

- gate 注册表的全局锁只用于取得/创建 `Arc`，随后立即释放；真正 ADB/SQLite 流程只持目标 serial 的 gate。
- start/apply/stop/emergency/status/endpoint reconciliation/reconnect/cleanup 对同一 serial 串行。status 与 endpoint 查询不是纯读取：它们可能写入 waiting/reconnected、向 Companion 重新 apply LAN endpoints 并持久化结果。
- 不同 serial 的操作不共享生命周期 gate，可以并行；底层 SQLite executor 可以短暂串行持久化，但不得包住 ADB 网络等待。
- `gate_for(serial)` 在 map lock 内清除无法 upgrade 的 Weak，再 upgrade 或创建目标 Arc，随后立即释放 map lock。操作完成后不保留强引用；即使用户不断尝试新 serial，expired entry 会在后续 lookup/reconciliation 被清理，map 不随历史 serial 无界增长。
- Application package cache 改为按 serial 索引；设备列表刷新时删除已不在线且没有运行记录的缓存项。缓存不是运行所有权来源。
- 容量预留必须先于 VPN 启动、reverse 创建或设备设置写入。后续失败按实际副作用决定精确删除准备行或保留为 `uncertain/cleanup_required`，不得释放不确定记录。

### Application 配置读写 gate

Infrastructure 的逐设备 gate 不能被 Application 的全局 `mutation_gate` 抵消。把“运行时读取配置”和“修改配置”建模为共享读/独占写 gate：

- Android start/apply/stop/emergency/status/endpoints 在整个 Application 用例期间持共享 read guard；A/B 可同时持有，因此不同 serial 的 ADB 操作不会互相串行。status/endpoints 必须覆盖 Workspace/profile 解析、Companion 调用、LAN endpoint reconciliation 和持久化完成。
- Workspace 切换、方案增删改、Listener 配置变化、reset 和其他会改变 activation 引用的操作持独占 write guard，等待在途 Android 操作结束。
- read guard 保证 start/apply 使用的 Workspace、profile、Listener 引用在完成前不被删除或改写；activation 仍是不可变快照。
- 不允许同时持 Application write guard 再调用会获取 read guard 的 public facade，避免自锁；内部 shutdown/transaction 路径直接调用已验证的 under-gate 方法。
- 现有其他必须全局串行的 mutation 可以保留单独 mutex，但不得包住 Android ADB await 并阻塞其他 serial。

### 统一锁顺序

所有路径遵守：`Application configuration guard -> canonical-sorted Environment resource gates -> one per-serial operation gate -> short registry snapshot/commit`。SQLite executor await 只能在不持 registry lock 时执行；同一操作可以持 per-serial gate 跨 SQLite await。禁止反序获取，禁止同时获取两个 per-serial gate；需要处理全部 owner 时按 serial 分解为独立任务。Environment apply 必须先取得 Application write guard，再确定并获取 canonical resource scope；shutdown 使用 under-gate 方法，不能递归获取 Application read guard。

## 运行生命周期

### 启动

1. 校验显式 serial 在线、方案有效、该 serial 未被管理。
2. 在目标 serial gate 内原子预留新 epoch；容量满在设备副作用前失败。
3. 执行该 serial 的 Reverse/LAN/device-only 准备和 Companion 调用。
4. 用 serial+epoch CAS 发布 `active` 或真实失败/不确定状态。

### 应用

1. 校验显式 serial+expected_epoch。
2. 捕获仅该 serial 的旧 reverse/endpoints/resume facts。
3. 执行两阶段替换；成功写入新 epoch，失败恢复或保留该 serial 的真实清理状态。
4. 不读取、停止或覆盖其他 serial。

### 停止与紧急恢复

- 只调用目标 serial 的 ADB 命令和 reverse cleanup。
- 完全确认清理后按 serial+epoch 删除记录。
- 设备不可达时只把该 serial 保持/转为 `waiting_reconnect` 或精确失败状态；其他 owner 保持不变。

### 断线与重连

- 设备列表 reconciliation 以 owner 快照和在线 serial 集合计算差异。
- 缺失 serial 仅通过该 serial+epoch CAS 进入 `waiting_reconnect` 并保存 resume_state。
- 同 serial 重连时在其 gate 内重新观测状态并恢复；同名以外的设备不能代替它。
- reconciliation 处理最多 8 条 owner，不依赖 UI selected device。

### 应用退出与恢复

- 正常 `app_shutdown` 取得 Application 独占 write guard，先读取按 serial 排序的全部 owner，再对每个 owner 独立调用 `network_stop(serial, epoch)`；最多 8 个 stop 全部必须尝试，单个失败不得短路其余设备。
- shutdown 可以并发执行不同 serial 的 stop，但错误汇总必须按 serial 稳定排序。Listener 只在所有 Android stop 尝试结束后停止，避免提前切断仍在清理的设备。
- 成功确认停止的 owner 被删除；设备不可达、清理失败或不确定的 owner 以真实状态保留。因此下一次启动恢复的是异常终止遗留记录以及正常退出时未能确认清理的记录，而不是已成功停止的设备。
- 强制退出/崩溃不运行 shutdown；启动时从 SQLite 恢复全部保留记录并逐设备 reconciliation。

## Application、诊断和环境配置消费者

- Android port/facade 全部显式目标化；singleton `runtime_owner` 和 singleton package cache 删除。
- Diagnostic report 将 `android_runtime_owner`/单 status 改为按 serial 排序的设备运行集合，并保留每项端点和错误。
- `EnvironmentValidatedApplyBaseline.android_owner` 改为 `android_owners: Vec<EnvironmentAndroidOwnerBaseline>`；每项必须包含非空 `profile_id`、原始 `serial`、`owner_epoch` 和非空 `state`，按 `(profile_id, serial)` 排序且键唯一。空 Vec 表示 inactive，不再使用一个 inactive sentinel。
- `EnvironmentApplyLeaseResourceKey::AndroidOwner` 使用 `{ profile_id: String, serial: String }`，删除可能碰撞的 `device_key: u64` 身份。`EnvironmentApplyLeaseResourceObservation::Android` 也包含精确 serial/epoch/state。
- baseline/lease 对 owner 集合做 set-diff：新增/变化发布当前 projection，删除发布 `None`；`observe_resource(AndroidOwner { profile_id, serial })` 只查精确键，缺失返回 inactive，不能返回其他 owner。
- Environment apply 在 Application write guard 下取得当前 owner 快照，canonical scope 是 baseline owner keys 与当前 owner keys 的并集，再与 listener/package keys 一起按现有资源比较器排序、去重、顺序获取。任何当前 owner 非空都返回现有 `AndroidOwnerMismatch`，不因 owner 数量、state 或 profile 放行；空 baseline 后新出现 owner 也返回该结果。
- Android owner mutation publishing 只发布目标 serial 的 before/after；apply 改变 profile 时先移除旧 `(profile, serial)` 键再发布新键。A 变化时 B 的 generation/projection 保持不变。
- MCP 保持只读并冻结唯一 wire：`android_package_list { serial } -> array`；`android_package_get { serial, package_name } -> object`；`android_network_status { serial } -> object`；新 `android_runtime_owner_list {} -> array`（按 serial 排序，删除旧 `android_runtime_owner`）；`android_network_endpoints { serial, profile_id? } -> object`。catalog required fields、backend argument DTO、dispatch 和 `OutputRoot` 必须逐项一致，不新增 mutation 能力。
- portable/reset 代码和测试引用新表名与集合语义。

### 配置引用与 destructive replacement 安全

- `device_network_profile_delete(profile_id)` 在 Application write guard 下读取全部 owners；只要任一 owner.profile_id 匹配，无论其 state 是 active、waiting_reconnect、uncertain、cleanup_required、stop_failed 或 faulted，都返回引用冲突且不删除。
- `workspace_delete(workspace_id)` 以该 Workspace 的全部 Android profile IDs 与全部 owners 求交；命中任何一项都拒绝。不得通过 selected serial、单次 status 或集合第一项判断。
- backup import、portable restore、environment/full configuration replacement 等会替换当前 Workspace/profile 集合的路径，只要 owner 集合非空就拒绝 replacement，且 configuration/protocol package store 零写入；不根据 candidate 是否恰好包含同名 profile 猜测兼容。
- data reset 是唯一会主动清理 owner 后继续 destructive replacement 的路径：在 Application write guard 下先按全 owner shutdown 合同尝试停止全部设备；任何 owner 未确认清理时，reset 返回错误，不调用 configuration/protocol-package/certificate reset，也不删除失败 owner 或其 profile。已确认停止的设备可以按正常 stop 合同删除自身 owner。
- 只有重新读取 `runtime_owners()` 确认空集合后，data reset 才执行现有原子 reset。reset 存储失败保持其既有错误语义，不伪造设备仍运行。

## UI 与异步状态

- 页面数据源是 `online devices ∪ runtime owners`，以 serial 去重。离线 owner 仍显示方案、状态、epoch、端点和断线原因。
- selected device 只控制当前编辑/展开上下文；每个卡片/行 action 的闭包捕获自身 serial，运行 mutation 同时捕获渲染时 epoch。
- 每台设备独立保存当前方案选择；owner 已存在时以 owner.profile_id 为运行事实，不被切换其他设备方案覆盖。
- package/status/endpoints query key 至少包含 serial；运行态 query key 包含 serial+epoch。
- 事件 `entityId` 使用 serial，payload 使用唯一 `runtime_epoch` 字段；接收方只更新匹配 serial 且 epoch 与当前 owner 相等的缓存。终止事件只在其 epoch 等于当前缓存 epoch 时清除；没有 owner 的查询使用 `serial + none` 键。
- 离线设备仍展示完整动作位置，但依赖 ADB 的动作禁用并显示“等待同序列号重连”；不会提供删除/放弃 owner 动作。
- 8 条记录已满时，未管理设备的 Start 禁用并显示容量原因；既有 8 台的 status/apply/stop/emergency 不受影响。

## 兼容与删除策略

- 不保留 singleton API、无 serial IPC 或 UI foreign-owner 阻断逻辑。
- 不迁移旧数据库，也不增加 v20 专用代码。
- Android Companion 收到的单设备命令内容保持不变，变化仅在桌面端把每次调用路由到显式 serial。
- `.omx/plans/prd-todo-remediation.md` 继续作为历史 R02 证据，不回写；新架构文档说明本任务替代该单 owner 产品合同。

## 实施顺序

1. 回归锁：新增失败测试证明显式 serial、集合、8/9 容量与跨设备隔离目标；保留旧行为证据但把旧断言转为新 RED 契约。
2. 冻结 Application/Host DTO、错误码、集合 view model 和生成绑定。
3. 实现 fresh Schema、`BEGIN IMMEDIATE` 集合 CRUD、容量 trigger 与 serial+epoch CAS。
4. 将 ADB owner/reverse/runtime/endpoints 改为注册表与逐设备 gate。
5. 将 Application 配置 gate 改为共享 read/独占 write，冻结 status/error epoch wire，更新全 owner shutdown、profile/workspace delete、full replacement/reset、诊断、Environment Vec/set-diff、精确 MCP read-only wire 和其他集合消费者。
6. 实现逐设备 UI、query/event 隔离和最多 8 台展示。
7. 更新架构/用户文档，保存测试证据，执行整体对抗审查。

## 风险与缓解

| 风险 | 缓解与证明 |
| --- | --- |
| 两个并发 start 绕过 8 台上限 | SQLite 写事务/约束做最终 admission；8→9 并发测试证明仅一个可成功 |
| A 的迟到 stop 清除 A 新 epoch | 所有清除/替换使用 serial+expected_epoch；stale epoch 测试 |
| A 操作覆盖 B 内存事实 | 注册表按 serial 更新，禁止 whole-state replacement；交错执行测试 |
| A status/endpoints 与 A apply/stop 交错写入 | 所有隐式写查询进入同 serial gate；受控交错和 stale epoch 测试 |
| 全局锁让多设备名义并行、实际串行 | 移除 `network_operation` 全局 gate；阻塞 A 时 B 状态/停止仍完成的测试 |
| Application `mutation_gate` 继续包住 ADB await | 配置共享 read/独占 write gate；A/B 并行与配置写等待测试 |
| 第 9 台失败后留下设备副作用 | admission 必须在任何 ADB mutation 前；fake runner 调用序列断言 |
| UI selected serial 变化导致命令串台 | 所有 mutation 参数显式 serial+epoch；late-response 和 switch-selection 测试 |
| 同 serial 迟到 status 无法判新旧 | status/event 冻结 `runtime_epoch` 语义；终止响应按 epoch 条件清理 |
| 失败被包装成 status 成功 | 失败只走 `Err(AppError)`；仅持久化权威 epoch 可进入错误关联字段 |
| 断线记录被当作空闲容量 | 所有非删除 owner state 均计数；waiting_reconnect 容量测试 |
| 集合消费者暗中只取第一项 | diagnostics/environment/MCP 多 owner 契约测试与静态搜索 |
| shutdown 只停一个或首个失败后短路 | 全 owner 独立 stop、稳定错误聚合、成功删除/失败保留测试 |
| 任意失败 serial 让 gate map 增长 | Weak gate registry 与 expired-entry prune 测试 |
| 离线/第二个 owner 的 profile 被删除 | 所有 delete/replace 检查完整 owner 集合，任意 state 都保护引用 |
| reset 先删配置再发现设备未清理 | 先尝试全 owner stop，重新确认空集合后才允许任何 reset store 写入 |

## 验收标准

1. A 运行 PA 时，B 可运行 PB，A 的 owner、端点、reverse 和方案不变。
2. A/B 的完整管理命令都只访问显式 serial。
3. 三种模式任意组合可由不同设备同时持有。
4. A 断线只改变 A；B 仍可 start/apply/status/stop。
5. 同 serial A 重连只恢复 A。
6. 正常退出尝试停止全部 owner，成功项删除、失败项保留；崩溃或重启恢复最多 8 条保留记录及其 cleanup/resume/endpoints facts。
7. 第 9 条记录在设备副作用前以稳定容量错误失败。
8. stale epoch 不能修改同 serial 新 epoch 或任何其他 serial。
9. fresh current schema 直接创建集合表；旧版本数据库按现有重建合同处理，无迁移分支。
10. status/event DTO 提供确定的 runtime epoch；UI 显示在线/离线运行设备并提供完整逐设备管理，缓存和事件无跨设备或同设备旧 epoch 污染。
11. profile/workspace delete、full replacement/reset、诊断、Environment `android_owners` Vec、精确 MCP read-only wire 全部消费完整集合；任意保留 owner 的配置引用不可被删除。
12. 目标 Rust/前端/绑定/静态/构建门禁通过；真实多设备环境不可用时明确 `NOT_RUN`。

## ADR

- 决策：采用按 serial 的 bounded registry + per-device gate + Application 配置 RW gate + SQLite serial/epoch CAS/容量 trigger。
- 原因：这是同时满足跨设备并行、错误隔离、重启恢复和陈旧操作保护的最小正确结构。
- 代价：公共合同和消费者同步修改范围较广；旧数据库被重建。
- 被拒绝：常驻 per-device actor（复杂度过高）；全量 JSON 快照/全局锁（隔离和并行不足）。
- 后续：若未来超过 8 台或需要远程 fleet/batch，再单独评估 actor/supervisor，不在本任务预埋。

## 执行编组建议

- Wave 1（串行）：`executor` 冻结 Application/Host/Schema 合同与测试；共享 DTO/绑定未稳定前不并行。
- Wave 2（可并行）：一个 `executor` 负责 Infrastructure/SQLite/ADB 生命周期；另一个 `executor` 负责前端逐设备 UI。两者不得同时修改生成绑定或 Application 共享类型。
- Wave 3（串行）：主 Agent 集成 diagnostics/environment/MCP/reset、运行全量验证；`code-reviewer` 做整体对抗审查，`verifier` 复核证据。

## 停止条件

只有 12 条验收标准有当前源码与可重复测试证据、整体审查为 APPROVE/CLEAR、任务文档与证据索引一致时才能完成。真实两台及以上设备不可用不阻止桌面自动化完成，但对应真机结论必须标记 `NOT_RUN`。
