# ADB 或桌面控制失联后可选自动关闭 Android VPN

## 任务信息

- 任务 ID：`TASK-20260828-004`
- 状态：`已完成`
- 任务日期：`2026-08-28`
- 创建时间：`2026-08-28 11:30:33 +08:00`
- 开始时间：`2026-08-28 13:59:23 +08:00`
- 最后更新时间：`2026-08-28 21:39:24 +08:00`
- 完成时间：`2026-08-28 21:39:24 +08:00`
- 创建路径：`docs/tasks/pending/2026-08-28/android-vpn-stop-on-adb-device-missing.md`
- 归档路径：`docs/tasks/completed/2026-08-28/android-vpn-stop-on-adb-device-missing.md`
- 关键词：`Android VPN`、`ADB devices`、`USB 拔出`、`桌面 App 退出`、`控制租约`、`心跳`、`TUN`、`WaitingReconnect`
- 任务优先级：`高`
- 优先级理由：涉及 Android VPN/TUN 生命周期、ADB 控制通道、桌面与设备端并发状态、超时和故障恢复；错误实现可能让目标应用持续断网或在用户预期停止后继续接管流量，并且需要真实设备拔线验证。

## 背景与目标

当前桌面端发现运行设备不再在线时，只把对应 runtime owner 标记为 `WaitingReconnect`。设备从
`adb devices` 消失后，原 ADB 控制通道已经不可用，桌面无法再补发 `stop`；Android Companion 也没有
持续控制租约，因此设备端 VPN/TUN 会继续运行。ADB reverse 模式下代理端点已不可达，但单个连接失败
不会触发整个 Android VPN 数据面退出，目标应用可能保持被 VPN 接管但无法完成代理连接。

目标是为每个 Android 网络方案增加可选的自动保护策略：当目标设备连续 5 秒不再出现在桌面的
`adb devices` 结果中，或者桌面 App 正常退出、异常退出或与设备端失联连续 5 秒时，由 Android 设备端
主动关闭该设备的 VPN/TUN。此策略默认开启；用户关闭后，以上控制失联不再自动停止 VPN。

## 范围

- 在 Android 网络方案中增加“ADB/桌面控制失联后自动关闭 VPN”开关，默认开启。
- 默认值同时用于新建方案以及缺少该字段的持久化/导入方案，避免未显式配置时继续运行 VPN。
- 桌面端按运行设备的 `serial + epoch/generation` 维护独立控制租约或心跳。
- 目标设备连续 5 秒未出现在 `adb devices` 结果中时，让对应设备端租约自然到期并关闭 VPN/TUN。
- 桌面 App 正常退出、异常退出或心跳停止连续 5 秒时，同样由设备端关闭 VPN/TUN。
- 设备端超时处理必须只作用于当前 generation，迟到心跳不得延长或停止新的运行实例。
- 自动停止后，桌面重连并查询设备真实状态时必须正确收敛 runtime owner，不得错误恢复已停止实例。
- 覆盖 device-only、LAN 和 ADB reverse 三种运行模式；触发条件是 ADB/桌面控制租约失联，不以代理端点是否仍可达作为替代判断。

## 不在范围

- 不因一台 Android 设备失联而停止、删除或重启桌面 Listener。
- 不影响其他 serial 对应的 Android 运行实例、ADB forward/reverse 或 runtime owner。
- 不把 `adb devices` 中仍存在但状态为 `offline`、`unauthorized` 等条目擅自等同于“设备不在列表”；如需扩展触发条件另行确认。
- 不改变现有手动停止、紧急恢复、系统撤销 VPN 授权和原生数据面故障的 fail-open 合同。
- 不新增未确认的重试、无限宽限期、代理降级或自动恢复路径。
- 不因控制失联自动关闭桌面代理，也不改变 Listener 共享和引用关系。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-08-28 11:30:33 +08:00` | 用户要求只针对目标设备不再出现在 `adb devices` 中时自动关闭 Android 端 VPN，并提供可选开关。 |
| `2026-08-28 11:30:33 +08:00` | 用户确认开关默认开启；关闭开关后允许 VPN 在 ADB 断开后继续运行。 |
| `2026-08-28 11:30:33 +08:00` | 用户接受连续 5 秒未检测到设备后关闭 VPN，避免瞬时 ADB 抖动直接触发停止。 |
| `2026-08-28 11:30:33 +08:00` | 用户进一步确认桌面 App 不再运行时也停止 VPN，因此可以把桌面心跳/控制租约丢失作为设备端停止依据。 |
| `2026-08-28 11:30:33 +08:00` | 自动停止仅作用于对应设备 VPN/TUN，不停止桌面 Listener，不影响其他设备。 |

## 未确认事项

无。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`
- 范围与不在范围：`PASS`
- 输入、输出与状态变化：`PASS`，开关开启时连续 5 秒控制失联关闭当前设备 VPN/TUN；关闭时保持运行。
- 错误行为：`PASS`，超时只处理当前 generation，不能用迟到心跳影响新实例，也不能把失败报告为停止成功。
- 具体示例：`PASS`，A920MAX 正在运行且开关开启，拔掉 USB 后连续 5 秒未出现在 `adb devices` 中，A920MAX 主 TUN 必须关闭；其他设备和 Listener 保持原状态。
- 可重复 PASS/FAIL 验收：`PASS`，可通过可控时钟、模拟心跳和真实设备拔线分别验证。
- 改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-08-28 13:59:23 +08:00`。

## 问题与根因分析

### 实际现象与环境

- Android VPN 已由桌面通过 ADB 控制启动。
- 用户拔掉 USB 后，目标设备从 `adb devices` 中消失。
- 桌面 runtime owner 进入 `WaitingReconnect`，设备端 VPN/TUN 仍可能继续运行。
- ADB reverse 已失效时，命中透明代理路由的连接会失败，但单个连接失败不会让整个 VPN 数据面退出。

### 预期行为及依据

- 默认策略开启时，目标设备或桌面控制连续失联 5 秒后，Android 设备端主动关闭当前 VPN/TUN。
- 策略关闭时保留现有继续运行行为。
- 停止只作用于对应 serial/current generation，不得停止共享 Listener 或其他设备。

### 最小复现

1. 保持桌面 App 运行，通过 ADB 为目标设备启动 Android 网络方案，确认 VPN/TUN 正在运行。
2. 确认自动关闭开关处于默认开启状态。
3. 拔掉目标设备 USB，使该 serial 不再出现在 `adb devices` 中。
4. 当前结果：桌面 owner 进入 `WaitingReconnect`，Android VPN 不因 ADB 消失而自动关闭。
5. 预期结果：连续失联满 5 秒后，目标设备主 TUN、原生数据面和 VPN Service 按既有停止顺序关闭。
6. 保持设备连接但退出或终止桌面 App，预期产生相同的 5 秒租约超时结果。
7. 关闭策略开关后重复上述步骤，预期 VPN 继续运行。

### 当前已验证

- 桌面设备协调在 serial 不在线时只调用 `mark_owner_waiting_reconnect()`，不发送停止命令。
- Android Companion 控制服务当前采用单请求、单响应连接，仅支持 start/apply/stop/emergency_restore/status，没有持续心跳或租约。
- Android VPN Service 会在明确停止、系统撤销或原生数据面整体故障时关闭 TUN，但没有 ADB/桌面控制失联看门狗。
- SOCKS5 单会话上游不可达不会标记整个 VPN 数据面故障，因此 ADB reverse 消失不会自动触发 fail-open。
- 当前 `AndroidNetworkProfile` 没有控制失联自动关闭字段。

### 推断

- 桌面只有在 ADB 设备已经消失后才能观察到列表变化，此时无法再可靠发送 `stop`；正确关闭动作必须由设备端基于此前建立的控制租约自行完成。
- 在现有单请求控制协议上增加 generation-aware 心跳，比把所有控制请求改成长连接更接近最小改动，但仍需在实现前用测试锁定并发和超时边界。

### 未知

- 真实 Android 设备在省电、前后台切换和短暂 ADB 抖动条件下的定时精度，需要真实设备测试确认，不能用单元测试结论替代。

### 已确认根因与影响范围

- 根因：桌面与 Android Companion 之间没有持续控制租约；设备离开 ADB 后桌面失去停止能力，而设备端没有独立的失联停止条件。
- 影响：Android Profile/序列化合同、桌面控制协议与后台心跳、设备端控制服务和 VPN 生命周期、runtime owner 重连收敛、三种运行模式及相关自动化和真实设备测试。

## 最小改动与最优设计比较

| 方案 | 设计与影响 |
| --- | --- |
| 最小改动 | 在现有版本化控制协议增加 heartbeat/lease renew 操作；桌面为每个活动 serial + epoch 定时续租，Android 为当前 generation 维护 5 秒看门狗；Profile 增加默认开启的布尔策略。保留当前 start/apply/stop/status 和 Listener 生命周期。 |
| 最优设计 | 把控制可用性建模为明确的设备端 lease，由桌面后台 owner 调度器统一续租，并让启动、应用、停止、重连和 shutdown 都通过 generation-aware 状态机管理租约。UI 只配置策略，不承担轮询或生命周期所有权。 |

优先以最小改动验证。它与现有 per-serial owner、epoch/generation 和 fail-open 停止顺序一致；若实现发现
心跳散落在页面 Hook 或多个控制入口，将改为后台 owner 调度器统一持有，避免 UI 生命周期决定 VPN 安全策略。

## 小任务列表

| ID | 任务 | 依赖 | 可并行 | 状态 | 验收 |
| --- | --- | --- | --- | --- | --- |
| AVAD-01 | 用可控时钟锁定默认开启、5 秒到期、关闭策略和 generation 竞态测试 | 无 | 否 | 已完成 | 4 类状态转换均有失败优先回归 |
| AVAD-02 | 扩展 Profile、持久化/导入合同和前端开关 | AVAD-01 | 否 | 已完成 | 新建及缺字段方案默认开启，用户选择可稳定保存 |
| AVAD-03 | 增加桌面 per-serial 心跳和控制协议租约续期 | AVAD-02 | 否 | 已完成 | 每个活动 serial/epoch 独立续租，页面未打开时仍工作 |
| AVAD-04 | 增加 Android generation-aware 5 秒看门狗和安全停止 | AVAD-03 | 否 | 已完成 | 失联关闭主 TUN；迟到心跳不影响新实例 |
| AVAD-05 | 收敛 owner 重连状态并执行自动化、拔线和桌面退出验收 | AVAD-04 | 否 | 已完成 | 自动化与多设备隔离通过；真实拔线和桌面异常退出保持 NOT_RUN |

## 测试计划

- Rust/domain：Profile 默认值、序列化/反序列化、导入缺字段行为和校验测试。
- 桌面 application/infrastructure：每 serial + epoch 心跳调度、设备列表消失、桌面 shutdown、心跳错误和取消清理测试。
- Android JVM/仪器测试：可控时钟驱动 4.999 秒不停止、5 秒到期停止、策略关闭不停止、旧 generation 超时/心跳不影响新实例。
- 生命周期测试：关闭主 TUN后再停止原生数据面和 Service，重复超时幂等，手动 stop 与超时并发不重复释放。
- 多设备测试：A 失联只停止 A；B 的心跳、VPN、Listener 和 owner 不变。
- 模式测试：device-only、LAN、ADB reverse 均按控制租约策略停止；不以代理端点健康替代控制租约。
- 真实设备：运行 VPN 后拔掉 USB，保存 `adb devices` 时间线、桌面日志、Android 日志、VPN 状态和 5 秒边界；另测正常退出桌面 App、强制结束桌面 App和关闭策略。
- 前端：开关默认开启、持久化、重新加载和可访问性标签测试。

## 对抗审查计划

- 检查是否错误地在设备消失后仍尝试通过 ADB 发送 stop，并把不可达误报为成功。
- 检查心跳是否绑定页面 Hook、当前选择设备或全局单例，导致离开页面、切换设备或多设备时失效。
- 检查旧 generation 的定时器或迟到心跳是否续租/停止新运行实例。
- 检查桌面退出顺序是否提前停止心跳但没有给设备留下完整 5 秒安全关闭窗口。
- 检查是否停止共享 Listener、影响其他 serial，或把代理连接失败错误当作控制失联。
- 检查 Android 定时器受休眠影响时是否仍满足真实设备合同；无法保证时必须报告验证缺口，不得扩大宽限或静默继续。

## 文档影响

- 更新 Android VPN 透明路由架构中的控制通道、租约所有者、停止条件和重连状态。
- 更新需求基线和用户操作说明中的开关含义、默认值、5 秒行为及 Listener 不受影响边界。
- 若控制协议版本或字段改变，同步协议说明、生成类型和兼容性测试。

## 实施记录

- `2026-08-28 11:30:33 +08:00`：完成当前代码只读分析并登记任务；确认默认开启、5 秒超时、ADB 设备消失和桌面 App 失联均触发对应设备端 VPN/TUN 自动关闭。本轮未修改生产代码。
- `2026-08-28 13:59:23 +08:00`：开始实现 Profile 策略、heartbeat v2、桌面逐设备续租和 Android generation 看门狗。
- `2026-08-28 16:40:00 +08:00`：补齐旧代超时与新代启动交错、入队失败回滚、租约起算点和 ADB forward 取消清理回归。
- `2026-08-28 21:39:24 +08:00`：完成自动化、独立复审、证据与本地交付门禁；真实设备破坏性场景按环境边界保持 NOT_RUN。

## 修改文件

- `android-companion/app/src/main/java/com/interceptproxy/vpn/`：控制协议、租约协调器、看门狗、Service 与 generation 状态机。
- `src-tauri/crates/domain/src/android_network.rs`：Profile 策略字段与默认值。
- `src-tauri/crates/infrastructure/src/adapters/android_adb/`：逐设备心跳、超时、取消安全 forward 清理和结构化错误。
- `src/features/android-network/`：默认开启开关及多设备 UI 回归。
- `docs/architecture/android-vpn-transparent-routing.md`、`docs/user-operation-guide.md`：租约所有权与用户操作说明。

## 附加文件

- [ANDROID-CONTROL-LEASE-001](../../testing/evidence/2026-08-28/TASK-20260828-004/ANDROID-CONTROL-LEASE-001/README.md)

## 验收结果

- `PASS_WITH_NOT_RUN`：自动化合同、取消清理、多设备隔离和独立复审通过；真实设备拔线与桌面异常退出未执行。

## 测试结果

- Kotlin 控制租约/协议/generation `17/17`。
- Rust heartbeat `1/1`、forward/取消清理 `11/11`、lease 隔离 `3/3`、Android engine `47/47`。
- Android 前端 `32/32`；严格静态检查、格式、类型、源码大小和生成绑定通过。
- Android Gradle 离线插件解析与真实设备破坏性场景为 `NOT_RUN`。

## CI 情况

- `PENDING`：本地门禁已完成；外部 Windows 流水线将在本批任务发送后执行。

## 完成总结

- 已实现默认开启、可关闭的控制失联保护。桌面按 serial/epoch 独立续租，Android 仅对当前 generation 在连续 5 秒失联后关闭 VPN/TUN；旧代回调、取消清理和一台设备失败不会影响新代或其他设备。
