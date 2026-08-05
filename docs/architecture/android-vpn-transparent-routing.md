# Android VPN 与透明代理路由

本文说明 Intercept Proxy 如何在不修改业务 App URL、Host 和端口的前提下，只接管指定 Android 应用的网络，并把目标连接透明转发到桌面 Listener；同时说明弱网注入、ADB reverse、运行指纹和 fail-open 的职责边界。

## 1. 总体链路

业务 App 仍访问原始 Server。Android Companion 通过 `VpnService` 只接管用户选中的应用，Rust 数据面从 TUN 读取连接后分别判断“去哪里”和“是否施加弱网”。

```text
目标应用
  -> Android VpnService allowlist
  -> TUN
  -> JNI / Rust DataPlane
  -> FailOpenEngine
  -> tun2proxy
  -> 进程内 SOCKS5
  -> 命中 proxy_routes：ADB reverse 或 LAN 端点 -> 桌面 Listener
  -> 未命中 proxy_routes：protect(fd) -> 原始目标
```

非目标应用、Android 系统流量、ADB 和 Companion 自身不进入该 VPN。

## 2. 控制面与数据面分离

### 2.1 桌面控制面

桌面端负责：

1. 选择设备和目标应用；
2. 校验 Profile、Workspace 与 Listener；
3. 把可移植的 `proxy_routes` 解析成当次运行端点；
4. 建立 ADB forward/reverse；
5. 通过版本化、长度前缀 JSON 协议发送 `start`、`apply`、`stop`；
6. 持续核对设备回报的状态与运行指纹。

控制通道使用：

```text
adb forward tcp:<临时端口> localabstract:intercept_proxy_vpn
```

Companion 的 LocalSocket 服务只接受 `shell` 或 `root` peer UID。Activity 被唤醒仅表示命令已送达，不能作为 VPN 已运行的证据。

### 2.2 Android 数据面

Kotlin 只负责 Android 平台边界：

- `VpnService` 生命周期和授权；
- `addAllowedApplication()`；
- 创建、关闭和移交 TUN fd；
- 前台通知；
- JNI 调用和 socket `protect()`。

Rust 负责：

- Profile 与运行配置校验；
- TCP/UDP 转发；
- 透明代理路由匹配；
- 弱网调度；
- 运行统计、故障和状态。

Kotlin 不维护第二套规则语义，避免桌面 Rust 与 Android 实现产生漂移。

## 3. 只接管指定应用

Companion 为 Profile 中的包名调用 `VpnService.Builder.addAllowedApplication()`。启动前 Rust 使用设备当前包清单重新校验：

- 包必须仍然安装；
- UID 必须和保存快照一致；
- shared UID 必须整组选择并确认；
- Companion 自身不能加入目标列表；
- 一个 Profile 最多选择 64 个包。

应用签名只用于展示和诊断，不作为启动阻断条件。UID 和 shared UID 才是 Android 路由隔离所依赖的运行事实。

## 4. `proxy_routes` 与 `destination_targets`

两者是独立维度。

### 4.1 `proxy_routes` 决定连接去向

每条透明代理路由包含：

- 原始目标域名、单个 IP 或 CIDR；
- 一个或多个明确端口；
- 当前 Workspace 中的 Listener ID。

启动时桌面端会：

1. 找到被引用的 Listener；
2. 确认 Listener 已启用且正在运行；
3. 解析域名的 A/AAAA 地址；
4. 为 USB 场景分配设备侧临时 reverse 端口；
5. 生成只属于本次运行的路由表。

命中的 TCP 连接被送到对应 Listener。未命中连接通过 `VpnService.protect(fd)` 访问原始目标。首版 UDP 不送入 HTTP Listener，而是保持原目标转发。

### 4.2 `destination_targets` 决定弱网范围

`destination_targets` 只决定哪些连接应用延迟、丢包、限速、乱序等弱网：

- 为空：目标应用的全部远端连接应用弱网；
- 非空：仅匹配的地址和端口应用弱网；
- 未命中：正常转发，不注入弱网。

配置透明代理不会自动扩大弱网覆盖范围，配置弱网也不会改变连接目标。

## 5. TUN 启动步骤

`InterceptVpnService` 的启动顺序是：

1. 接收 `ACTION_START`；
2. 校验 generation，拒绝 stop 之后迟到的旧启动请求；
3. 解析 Profile 与 proxy runtime JSON；
4. 调用 Rust 校验 Profile 和设备包清单；
5. 确认 VPN 授权有效；
6. 根据目标 UID 和 MTU 判断复用或重建 TUN；
7. 复制 TUN fd，`detachFd()` 后交给 Rust；
8. Rust 启动 `DataPlaneHandle`；
9. 数据面就绪后才发布 `running`。

Rust 数据面内部顺序：

```text
TUN fd
  -> ManagedTunFile
  -> TUN 双向 pump
  -> FailOpenEngine
  -> tun2proxy
  -> SOCKS5
  -> ProxyRouteTable
  -> SocketProtector.protect(fd)
```

对外 socket 必须先 `protect(fd)`，否则 Companion 发出的连接会再次进入自身 VPN，形成递归。

## 6. ADB reverse 两阶段切换

USB 场景中，设备通过如下映射访问桌面 Listener：

```text
设备 127.0.0.1:<device_port> -> 桌面 127.0.0.1:<listener_port>
```

为避免应用修改过程中撤销仍可能被设备使用的端口，桌面端采用两阶段切换。

### 6.1 Prepare

1. 锁定设备 serial；
2. 读取旧 reverse ownership；
3. 先解析全部域名；
4. 分配不冲突的设备端口；
5. 创建新 reverse；
6. 生成 proxy runtime 和运行指纹；
7. 记录 prepared facts。

准备失败时清理本轮新映射；清理失败的端口仍保留 ownership，供后续停止或紧急恢复继续处理。

### 6.2 Commit、Rollback 与 Uncertain

- 设备进入匹配本次指纹的 `Running`：提交新映射，再清理旧映射；
- 设备明确拒绝且尚未接受切换：回滚新映射；
- 设备已接受但桌面超时、无法确认最终状态：同时保留新旧映射，标记为不确定态并返回可重试错误。

不确定态不能立即删除新映射，否则设备可能在桌面超时后才完成切换，并连接到已被撤销的端口。

## 7. 运行指纹

桌面端和 Companion 共同核对：

- `profile_fingerprint`：稳定 Profile JSON 的指纹；
- `route_fingerprint`：实际运行路由、临时 host/port 和解析结果的指纹；
- `route_count`：透明代理路由数量。

只有 state、Profile ID、两个指纹和 route count 全部匹配，桌面端才把状态视为当前方案的已验证 `Running`。

## 8. 启动、应用修改、停止与紧急恢复

### 8.1 启动和应用修改

Application 层先持有 mutation gate，防止 Workspace、Listener 或 Profile 在操作中并发变化；ADB adapter 再用 network-operation 锁串行化设备网络变更。然后依次执行校验、prepare、发送命令、确认 `Running` 和 commit/rollback/retain。

只改弱网参数且目标 UID、MTU 不变时可以复用 TUN，但 Rust 数据面仍会重启；启动失败则进入 fail-open。

### 8.2 停止

停止会推进 generation、请求 Companion 关闭 TUN，并且无论控制请求成功与否都尝试清理当前 reverse ownership。控制错误和清理错误会合并返回，避免隐藏残留资源。

### 8.3 紧急恢复

紧急恢复通过 ADB force-stop Companion，利用进程退出关闭 TUN fd，再清理所有已知 reverse 端口。它用于控制 socket 不可信或普通 stop 无法完成时优先恢复设备网络。

## 9. Fail-open

系统不是 kill switch。Profile 无效、授权失效、TUN/JNI/Rust/SOCKS5 异常、运行指纹不一致或原生数据面退出时，Companion 会关闭 TUN，让目标应用恢复系统网络。

关闭顺序强调恢复速度：Kotlin 先关闭主 TUN，使 Android 立即撤销 UID 路由；随后 Rust 在有限时间内停止运行时并释放副本。原生线程不能无限阻塞 Android Service 主线程。

含透明代理路由的 Profile 不自动恢复，因为 ADB reverse 和 LAN 端点属于当次运行事实，重启后必须由桌面重新解析并建立。

## 10. 证据边界

验证结论必须分层记录：

1. 源码事实：协议、状态机、TUN/JNI/Rust 实现；
2. 单元测试：校验器、路由表、状态迁移和弱网决策；
3. 模拟器门禁：ADB reverse、本地 upstream、Listener 与 VPN 联动；
4. 真机网络门禁：指定应用接管、非目标应用隔离、ADB 存活和停止恢复；
5. 真实业务验收：真实设备、真实证书、真实上游和真实业务响应。

Listener 显示运行只证明本地端口已监听；VPN 显示 `Running` 只证明设备进入匹配指纹的运行态。两者都不能单独证明真实业务请求成功。
