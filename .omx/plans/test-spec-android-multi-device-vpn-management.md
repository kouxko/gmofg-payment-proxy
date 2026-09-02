# Test Spec: Android 多设备 VPN 并行运行与逐设备管理

状态：`APPROVED_FOR_IMPLEMENTATION`（Architect APPROVE；Critic Review 3 APPROVE）

关联任务：`TASK-20260827-001`

## 验证原则

- 先证明身份和所有权隔离，再证明多设备成功路径。
- 所有 mutation 测试记录目标 serial、expected epoch、ADB 调用序列和持久化前后快照。
- 生命周期测试覆盖竞争、取消、断线、重连、失败、重启与清理，不只覆盖成功。
- 8 台上限在设备副作用前验证；所有非删除 owner state 都计入容量。
- 不执行旧 Schema 数据迁移测试；只验证 fresh schema 与项目既有旧库重建合同。

## Stage 0：当前行为与新 RED 合同

1. 保存现有单 owner 测试输出作为变更前证据：B 被 A 阻止、A 断线后保留。
2. 新增 Application/Host 编译级测试：所有完整管理命令必须传 serial；apply/stop/emergency 必须传 expected epoch。
3. 新增集合契约测试：runtime owners 返回稳定排序 Vec，不是 Option。
4. 将“foreign owner 阻止 B”前端测试改为“B 独立可启动且 A 不变”的 RED 测试。
5. 新增 fake ADB runner 断言：切换 selected device 后，已提交命令仍使用其显式 serial。

## Application 与 IPC 合同矩阵

| 场景 | 预期 |
| --- | --- |
| install/update/consent(serial=B) | 只执行 `adb -s B ...` |
| package list/query/refresh(serial=A) | cache key 和 runner 只属于 A |
| start(serial=A, PA) | 无 A owner 时创建新 epoch |
| start(serial=A) 且 A 已存在 | `ANDROID_RUNTIME_ALREADY_MANAGED`，无 ADB mutation |
| apply(A, epoch1, PA2) | 仅 A 更新并产生 epoch2 |
| apply(A, stale epoch0) | `ANDROID_RUNTIME_EPOCH_STALE`，无 mutation |
| stop/emergency(A, epoch1) | 只访问 A，B/C 快照逐字段不变 |
| status/endpoints(B) | 只返回 B，不回退 selected/first owner |
| 第 9 台 start | `ANDROID_RUNTIME_CAPACITY_EXCEEDED`，无 ADB mutation |

对 Tauri command serialization 和生成 TypeScript 类型分别做正/负例；旧的无 serial 参数调用应编译或类型检查失败，不保留兼容 overload。

### Status/Event epoch wire

- Start 成功返回新 runtime_epoch；admission 前失败为 `Err(AppError)` 且 error.runtime_epoch=None。
- Apply 成功返回 successor epoch；stale apply 无设备 mutation，返回错误而非无 epoch 成功状态。
- managed Status 对 active/waiting/faulted 都返回当前 epoch；unmanaged stopped Status 返回 None。
- Stop/Emergency 成功响应保留 expected epoch 作为终止 epoch，前端只清除相同 epoch；失败响应关联仍持久化 owner epoch。
- 序列化和生成绑定固定字段名 `runtime_epoch`，拒绝缺失该字段的事件 fixture。
- 任何 Start/Apply/Stop/Emergency/status/endpoints 失败都保持 Rust `Err` 和 Tauri/MCP error wire；不出现 `Ok(Faulted)` 或其他成功外壳替代错误。
- 失败后已有持久化 owner 时，error.entity_id=serial 且 error.runtime_epoch 等于重新读取的权威行；admission 前/not-managed 错误 epoch=None。
- 注入 owner persistence 失败时，错误不得携带新生成但未落库的 epoch，也不得发布该 epoch 的成功/状态事件；旧行仍权威时只关联旧 epoch。

## SQLite fresh-schema 与 CRUD

- fresh DB 创建 `android_runtime_owners`，主键 serial、epoch 唯一、既有枚举 CHECK 约束完整。
- 保存 A/B/C 后按 serial 稳定读取，restart/reopen 逐字段相等，包括 reverse ports、resume_state 和 endpoints。
- 同 serial 保存不得绕过 expected epoch；replace 只匹配 serial+epoch。
- clear(A, epochA) 删除 A，B/C 完全不变。
- clear(A, stale) 返回 false/稳定 stale 映射，所有行不变。
- 8 条不同 serial 可保存；第 9 条被数据库最终不变量拒绝。
- 8 条已满时更新已有 serial 成功，不被 insert trigger/容量逻辑误拒绝。
- 两个并发 admission 从 7 条开始，只允许一个新 serial 成功；最终恰好 8 条。
- 直接 SQL/未来 store 调用尝试插入第 9 个新 serial 时由容量 trigger 拒绝并映射稳定错误；8 条已满时同 serial upsert 仍可执行。
- 任一 JSON/UUID/时间/enum 损坏时 `load` fail-closed，不跳过坏行并把其余行伪报为完整集合。
- SQLite 写入失败、trigger 注入失败和 transaction rollback 后，原集合逐行相等。
- 打开版本不匹配的开发数据库时执行现有全库重建合同并创建 fresh 多 owner 表；明确断言不走 singleton migration。
- portable/reset/workspace reset 删除全部 owner 行并保持其他既有 reset 断言。

## Owner registry 与并发矩阵

| 并发情形 | 证明点 |
| --- | --- |
| A start 阻塞于 fake ADB，B status | B 在有界时间内完成，证明无全局生命周期锁 |
| A apply 阻塞，B stop | B 正常停止且只清 B |
| A start 持 Application 配置 read guard，B stop | B 取得共享 read guard 并完成，不被旧 mutation mutex 串行 |
| A/B 运行操作在途，Workspace/profile/listener mutation | 配置 mutation 等待两个 read guard 释放后才执行，引用不在途中失效 |
| A endpoint reconciliation 阻塞，Workspace/profile/listener mutation | 配置 write guard 等待完整 endpoints 用例及持久化结束 |
| A status 正在恢复 LAN endpoint，B status | A/B 可并行；同一 A 的 apply/stop 必须等待 |
| A stop 与 A apply 同时提交 | 同 serial 串行，后执行者依据新 epoch 成功或稳定 stale |
| A disconnect 与 A stop 竞争 | 最终为合法单一 A 状态，不丢 cleanup facts |
| A reconnect 与旧 A poll 迟到 | 旧 epoch 不能覆盖重连后的新事实 |
| A 失败，B/C 正常 | B/C owner、端点、reverse、profile、错误全部不变 |
| 7 条记录时 B/C 并发 start | 只一条 admission 成功，失败设备无调用记录 |
| A apply 阻塞，A status/endpoints 提交 | A 查询等待同 serial gate，完成后观察新 epoch；不得执行迟到 LAN apply |
| A status/endpoints 阻塞，B stop | B 正常完成，证明隐式写查询没有恢复全局串行 |

测试不得依赖 wall-clock sleep 判定顺序；使用 barrier/channel 控制交错，并为每个等待设置有界 timeout。

### Gate 生命周期与锁顺序

- 连续对大量不存在 serial 发起失败操作后，Weak gate map 在 prune 后只保留 live entries，不随历史 serial 增长。
- 同 serial 并发取得同一 live Arc；操作结束且无强引用后 entry 可清理。
- instrumentation 断言锁序为 Application -> sorted Environment -> per-serial；反序测试必须失败或在结构上不可调用。
- registry lock 不跨 SQLite executor await；shutdown/environment under-gate 路径不递归申请 Application guard。

## 模式组合矩阵

至少覆盖以下同时运行组合，每项断言 serial、epoch、profile、reverse/endpoints 和 stop cleanup：

| A | B | C | 预期 |
| --- | --- | --- | --- |
| ADB Reverse | ADB Reverse | N/A | 设备命名空间独立；相同 device port 合法，记录不覆盖 |
| ADB Reverse | LAN | N/A | B 无 A reverse facts，A 无 B LAN endpoints |
| ADB Reverse | Device-only | LAN | 三者独立 start/status/stop |
| LAN | LAN | N/A | 各自 profile/endpoints 独立 |
| Device-only | Device-only | N/A | owner/epoch 独立且无虚假 reverse |

## 断线、重连与重启

- A/B active，A 消失：只 A 进入 waiting_reconnect 并保存原 resume_state；B 保持 active。
- A waiting 时 B 可 start/apply/status/stop，且容量仍包含 A。
- serial A 重连：只 A 触发 reconciliation；相似型号或不同 serial C 不能接管。
- A 重连失败：A 保持真实 fault/cleanup/waiting 状态，B 不变。
- 桌面重启恢复 1、2、8 条混合 state owner；每项 transition reason 更新为合法恢复语义且 cleanup facts 不丢。
- 重启后同 serial gate/CAS 仍防止 stale epoch。
- 8 条中 3 条 waiting 时第 9 条仍失败；确认停止任一既有记录后才释放一个容量位。
- 正常 shutdown 有 A/B/C 时全部 stop 都被调用；A 首个失败不阻止 B/C。
- shutdown 中 B 成功则 B 行删除，A 不可达则 A 行保留真实状态；错误列表按 serial 稳定排序。
- Android stop 尝试全部完成后才停止 Listener；测试记录调用序列。
- 强制终止模拟不执行 shutdown，reopen 后恢复全部持久化 owner；正常退出只恢复未确认清理项。

## Reverse 与清理隔离

- 所有 runner 命令包含精确 `-s <serial>`。
- cleanup A 只删除 A 的 reverse 映射；B 使用相同 device-side port 时仍存在。
- A reverse preparation 失败时只记录 A 的 remaining ports/cleanup_required。
- A emergency restore 不扫描、删除或覆盖 B owner facts。
- stop 返回成功前必须同时证明 Companion stop 与目标 serial reverse cleanup 的既有成功合同；部分成功保持 A 的失败状态。

## Package cache 与完整管理

- A/B package list 交错完成时各自 cache 内容正确。
- selected serial 从 A 切 B 时，A 的迟到 package query 不能写入 B。
- device list 移除无 owner 的 A 后清除 A cache；waiting owner 的 A cache/运行事实按合同保留或重新查询，但不得迁移给 B。
- install/update/consent 对 A 失败时只显示 A 错误；B action 和 cache 保持可用。
- query/refresh 不使用 singleton `Option<Vec<_>>` cache。

## 前端组件与异步隔离

- 在线 A/B 与离线 owner C 合并为 3 个按 serial keyed 的设备视图，无重复。
- 每个卡片显示独立连接状态、owner state、profile、epoch、endpoints 和错误。
- 点击 B Start 即使随后选择 A，IPC payload 仍为 B。
- 点击 A Apply 后返回旧 epoch 响应，当前 A epoch 已更新时旧响应被忽略/隔离。
- A status poll 迟到不能覆盖 B；query/event keys 包含 serial，runtime keys 包含 epoch。
- entityId=A 的事件只使 A query 失效；runtime_epoch 旧于 A 当前值的 payload 不降级 A。
- A epoch1 Stop 成功事件迟到，而 A 已以 epoch2 重新启动时，不清除 epoch2 卡片/缓存。
- managed 与 unmanaged status fixture 都显式包含 runtime_epoch（Some/None），缺字段 payload 被拒绝而非默认成 None。
- A 离线时 ADB-dependent actions 可见但 disabled，说明等待同 serial 重连；没有删除/接管按钮。
- 8 条已满时未管理设备 Start disabled 并显示容量；已有 owner 的 Apply/Status/Stop/Emergency 仍可用。
- 每台设备独立 profile selection；切换 B 不改变 A owner/profile 展示。
- 键盘导航、按钮名称、disabled 原因和状态播报满足现有可访问性门禁。

## 集合消费者

- Diagnostic report 输出 A/B/C 有序集合；不会只报告第一项。
- `EnvironmentValidatedApplyBaseline.android_owners` 是按 `(profile_id, serial)` 排序且唯一的 Vec；空 Vec 是 inactive，双 owner 不会只保留第一项。
- Environment baseline/lease 捕获完整集合；A 变化产生 A 的 set-diff，B projection/generation 保持不变。
- `EnvironmentApplyLeaseResourceKey::AndroidOwner` 使用原始 serial 而非 device hash；`observe_resource(AndroidOwner { profile_id: PA, serial: A })` 只返回 A；A 缺失时返回 inactive，即使 B 存在也不能返回 B。
- A apply 从 PA 变为 PA2 时发布旧键删除与新键增加；B 的资源 gate 不被获取或失效。
- canonical scope 包含 baseline/current owner key 并集，按统一 comparator 获取；当前存在任意 1..8 个 owner 都返回 AndroidOwnerMismatch，空 baseline 后新增 owner 也不能误放行。
- MCP 精确 wire 契约：`android_package_list` 必填 serial/Array；`android_package_get` 必填 serial+package_name/Object；`android_network_status` 必填 serial/Object；`android_runtime_owner_list` 空输入/Array；`android_network_endpoints` 必填 serial、可选 profile_id/Object。旧 `android_runtime_owner` 不在 catalog/backend allowlist，缺 serial 和未知字段均返回参数错误。

## 配置删除、替换与 reset

- owners=[A(PA,active), B(PB,waiting_reconnect)] 时删除 PB：因第二个/离线 owner 引用而拒绝，Workspace 与 owner 集合逐字段不变。
- 对 uncertain、cleanup_required、stop_failed、faulted 各 state 分别验证 profile 引用保护，不只测试 active。
- Workspace W2 的 profile 被 B 引用、W1 为 selected/第一项时删除 W2：拒绝，证明不依赖 selected/first owner。
- backup import、portable restore、full/environment configuration replacement 在任意 owner 非空时拒绝；configuration、protocol package、certificate/material store 的 replace/reset 调用计数均为 0。
- data reset 对 A/B/C 全部调用 stop；A 首个失败仍继续 B/C。失败 owner 保留，其 profile/config 保留，所有 destructive reset store 调用计数为 0。
- data reset 中所有 owner 确认停止并重新读取为空后，才执行一次既有原子 reset；不存在“先删 owner/profile，再把失败报告给用户”的路径。
- reset stop 部分成功时，成功设备 owner 可按 stop 合同删除，但配置写入仍为 0，失败设备 owner/profile 完整保留。
- 全仓静态搜索不再有业务代码调用 singleton `runtime_owner()`、`load_android_runtime_owner()` 或无 serial network command；历史 `.omx` 计划可保留。

## 错误与安全

- 验证稳定错误码：capacity、already managed、not managed、stale epoch、device unreachable、cleanup required。
- 错误包含安全的 serial/epoch/stage；不包含密钥、PIN、密码、证书私钥或完整敏感交易载荷。
- 一个设备错误不得以全局 toast/状态把其他设备标成失败；UI 错误归属 serial。
- 任何持久化失败不得返回启动/停止成功。

## 建议验证命令

先运行变更范围，再运行仓库门禁；实际 package 名以当前 workspace 为准：

```sh
pnpm vitest run src/features/android-network
pnpm typecheck
pnpm lint

cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application android
cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure android
cargo test --manifest-path src-tauri/Cargo.toml --workspace
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features -- -D warnings

pnpm bindings
cp src/generated/rust-types.ts /tmp/android-multi-vpn-rust-types-first.ts
pnpm bindings
cmp /tmp/android-multi-vpn-rust-types-first.ts src/generated/rust-types.ts

pnpm test
pnpm scan:source-size
pnpm scan:frontend-boundaries
pnpm build
git diff --check
```

若仓库已有 architecture/requirements 脚本覆盖 Android port、MCP catalog 或 Infrastructure 边界，必须加入并保存输出；不得用 workspace test 退出码代替逐项证据。

## 真机验证

最低真实资源为 2 台 Android/PAX 设备；理想资源为 3 台以覆盖混合模式。每个用例证据保存：

- 设备 serial、型号、ADB 状态、Companion 版本。
- 每台设备方案 ID、模式、端点、epoch、运行前后状态。
- Desktop 命令与事件、ADB reverse 列表、Companion 返回、实际 VPN 网络结果。
- A 断线、B 继续操作、A 同 serial 重连的完整时间线。
- 每台设备 stop/emergency 后的 reverse 与 owner 清理结果。

真机用例：

1. A/B 同时 ADB Reverse。
2. A=ADB Reverse，B=LAN。
3. A 断线时 B apply/status/stop，随后 A 重连。
4. 若有 3 台，A/B/C 三模式混合。

资源不可用时每项标记 `NOT_RUN`，保存从零重放步骤；自动化 PASS 不能替代真机结论。

## 证据目录

正式执行保存至：

```text
docs/testing/evidence/<执行日期>/TASK-20260827-001/<CASE-ID>/
```

每个用例至少包含 `README.md` 和 `metadata.json`，并按实际情况保存 inputs、outputs、steps、replay、日志和截图。共享工作区验收期间冻结被测文件与运行环境。

## 完成门禁

- Application/IPC/SQLite/registry/lifecycle/UI/consumer 适用测试全部 PASS。
- bindings 连续生成字节一致。
- fmt、clippy、typecheck、lint、architecture/static、build 与 workspace 受影响门禁有新证据。
- 独立 reviewer 对跨设备误操作、容量竞争、stale epoch、断线保留和迟到 UI 响应给出 APPROVE/CLEAR。
- 真机不可用项明确 `NOT_RUN`；不存在被描述为 PASS 的缺口。
