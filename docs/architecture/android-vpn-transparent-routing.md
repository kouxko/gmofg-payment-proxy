# Android VPN、TUN 与透明路由

本文说明桌面端、ADB、Android Companion、VpnService、JNI Rust 数据面和桌面 Listener 如何协作，
在不修改目标 App URL/Host/端口的情况下转发指定应用流量，并可独立注入弱网。

## 1. 总体链路

```text
目标 App
  -> VpnService allowlist
  -> TUN
  -> Kotlin 持有主 fd / JNI 把 dup fd 交给 Rust
  -> Rust 弱网双向 pump
  -> tun2proxy（Virtual DNS）
  -> 进程内 SOCKS5
       ├─ 命中 ProxyRouteTable -> ADB reverse 或 LAN -> 桌面 Listener
       └─ 未命中 -> VpnService.protect(fd) -> 原始 Server
```

非目标 App 不进入 TUN。Companion 自身禁止加入目标列表；所有对外 Socket 必须先 `protect(fd)`，
否则会再次进入自己的 VPN 形成递归。

## 2. 三个职责边界

### 2.1 桌面 Application/Infrastructure

桌面端负责：

- 选择设备、Workspace、Profile 和 Listener；
- 根据当前设备包清单校验目标应用和 UID；
- 把可移植 `proxy_routes` 解析成当次运行端点；
- 建立/清理 ADB forward 和 reverse；
- 通过版本化控制协议发送 start/apply/stop/status；
- 持久化 runtime owner，并核对运行指纹。

### 2.2 Kotlin Companion

Kotlin 只负责 Android 平台边界：

- VPN 授权、前台 Service 和通知；
- `addAllowedApplication()`；
- TUN 建立、复用、关闭和 fd 所有权；
- LocalAbstractSocket 控制服务；
- JNI 启停与 `VpnService.protect()`；
- 原生故障回调后在主线程关闭 TUN。

### 2.3 Rust Android 数据面

`android-engine` 负责 Profile/运行路由校验、弱网决策、TUN pump、tun2proxy、SOCKS5 路由和统计。
Kotlin 不复制弱网或透明路由语义。

## 3. Profile 与启动校验

Workspace 保存可移植 `AndroidNetworkProfile`：

- `target_applications`：包名与保存时 UID；
- `destination_targets`：弱网匹配 IP/CIDR 和可选端口；
- `proxy_routes`：原始 destination/ports 到 Listener ID；
- `confirmed_shared_uids`；
- `auto_resume_after_reboot`；
- `weak_network`。

每次启动使用当前安装清单 fail-closed 校验：

- 至少一个且最多 64 个目标 App；
- 包名合法、仍安装且 UID 未变化；
- Companion 自身未被选择；
- shared UID 必须整组选择并显式确认；
- destination/route 数量、CIDR/host、端口和重复项合法；
- 丢包概率、速率、MTU/MSS、位翻转和第 N 个 TCP flag 参数在范围内。

签名目前用于展示/诊断，不是启动阻断依据；Linux UID 才是 VpnService allowlist 的运行事实。

## 4. `destination_targets` 与 `proxy_routes`

两者互相独立：

- `destination_targets` 只决定哪些远端流量应用弱网；为空表示目标 App 的全部流量；
- `proxy_routes` 只决定哪些原目标送到桌面 Listener；未命中保持原目标转发。

配置透明路由不会自动扩大弱网覆盖，配置弱网也不会改变连接目标。

Workspace 里的 `proxy_routes` 不保存桌面 IP、临时端口或 DNS 结果。桌面启动时生成
`ProxyRuntimeConfiguration`，每条 resolved route 包含原目标、端口、桌面解析 IP 快照以及当前
ADB reverse/LAN 入口。

## 5. DNS 与路由匹配

TUN 把 DNS 指向本地基准地址，tun2proxy 使用 Virtual/Fake-IP DNS 在设备内回答。后续 SOCKS5
尽量携带原始域名，`ProxyRouteTable.for_domain()` 可精确命中。

域名真实 IP 由桌面启动时解析，并通过 `resolved_original_ips` 下发，用于兼容 App 已缓存 DNS、
直接连接真实 IP 的情况。Android 启动阶段不会依赖设备物理网络再次解析原域名。

数值 IP 的匹配顺序是：

1. IP/CIDR；
2. 桌面下发的域名解析 IP 快照；
3. 若同一端口只有唯一一条域名路由，可按端口补偿匹配；
4. 同端口多条域名路由时不猜测，保持未命中。

运行路由必须与 Profile 中 Listener/destination/ports 集合完全一致，否则数据面拒绝启动。

## 6. TUN 和 Rust 数据面启动

`InterceptVpnService` 的关键顺序：

1. generation 校验，拒绝晚到的旧 start；
2. 解析 Profile、运行路由并读取当前包清单；
3. Rust 启动前校验；
4. 确认 VPN 授权；
5. 目标包集合和 MTU 未变时可复用 TUN，否则先关闭旧 TUN；
6. Builder 配置 IPv4/IPv6 地址、全路由、Virtual DNS 和 allowlist；
7. Kotlin 原子持有主 TUN fd，并复制 fd 给 Rust；
8. Rust 创建弱网桥接 datagram、SOCKS5 和 tun2proxy 任务；
9. 所有核心任务至少被 poll 且无早期失败后报告 ready；
10. Companion 发布带指纹的 Running。

四个核心任务为 upload pump、download pump、SOCKS5 server 和 tun2proxy。任一异常退出都会通知
Kotlin fail-open。

## 7. ADB 控制与数据路由

### 7.1 控制通道：adb forward

桌面建立：

```text
adb forward tcp:<desktop-temporary-port> localabstract:intercept_proxy_vpn
```

控制帧是 4 字节大端长度 + JSON，协议版本为 1，最大 1 MiB，request/response 必须匹配 UUID。
操作白名单包含 profile、start/apply/stop/emergency_restore/status。设备 LocalSocket 服务仅接受
shell/root peer UID。

Activity 救援通道只能作为授权/恢复入口；“Intent 已送达”不等于 VPN 已运行。

### 7.2 数据通道：adb reverse

USB 模式为每条 Listener 路由建立：

```text
设备 127.0.0.1:<device-port>
  -> adb reverse
  -> 桌面 127.0.0.1:<listener-port>
```

LAN 模式使用当前可达桌面地址和 Listener 端口，不创建 reverse。桌面会检查 Listener 正在运行、
bind 地址、allowed client CIDR 和端点健康。

## 8. ADB reverse 两阶段更新

更新不能先删除旧映射。当前流程是：

### Prepare

1. 串行化同一设备网络操作；
2. 读取并持久化旧 runtime owner；
3. 解析全部目标与 Listener；
4. 分配不冲突设备端口并建立新 reverse；
5. 生成运行路由和 profile/route fingerprint；
6. 把 owner 标记为 cleanup-required/准备中。

### Commit / Rollback / Uncertain

- 设备确认匹配指纹的 Running：提交新 owner，再清理旧映射；
- 设备明确拒绝且未接受：回滚新映射；
- 命令已可能被接受但桌面超时或断联：同时保留新旧映射，标记 Uncertain；
- 清理失败：保留 ownership 和 CleanupRequired/StopFailed，后续恢复继续处理。

不确定态不能立即删除新端口，否则设备可能稍后完成切换并连接到已撤销映射。

## 9. 运行指纹与 owner

桌面与 Companion 共同核对：

- profile ID；
- `profile_fingerprint`；
- `route_fingerprint`；
- `route_count`；
- 设备 serial 和当前 generation/epoch。

只有状态和这些事实匹配才视为 verified Running。

SQLite 的 `android_runtime_owner` 保存模式（device-only/LAN/ADB reverse）、Profile、状态、来源、
reverse ports、runtime endpoints 和 transition reason。进程重启或设备重连时依据它继续清理/恢复，
而不是猜测 ADB 当前端口属于哪个运行实例。

## 10. Fail-open 与停止顺序

Android VPN 不是 kill switch。Profile 无效、授权失效、JNI/原生数据面异常、运行指纹不一致、TUN
任务退出或外部 stop 超时都优先关闭 Kotlin 持有的主 TUN，使 Android 立即撤销目标 UID 路由，然后
再停止 Rust 副本和后台任务。

停止顺序：

```text
close main TUN -> clear TUN configuration -> stop native runtime -> unregister receiver -> stop service
```

控制线程等待 Android 主线程超时时也可原子取得并关闭 TUN。普通 stop 失败时保留 owner；紧急恢复
可 force-stop Companion 并清理已知 forward/reverse。

只有不含透明路由的 Profile 可以保存自动恢复 activation。透明路由依赖当次 ADB reverse/LAN 与
桌面 DNS 快照，重启后必须由桌面重新 prepare。

## 11. 弱网数据面

Rust `FailOpenEngine` 对每个包生成确定性决策，支持延迟、随机/突发丢包、重复、乱序、限速、blackout、
DNS blackhole、指定 TCP flag 第 N 次丢弃、位翻转和 PMTU/MSS 行为。`destination_targets` 决定是否
应用这些动作。

无法解析或当前不支持的包形态保持原样通过，并增加未实施/诊断计数；不能因为观测或弱网引擎内部
错误永久阻断设备网络。统计按方向聚合，shared UID 不伪造成单包名统计。

## 12. 验证分层

1. Rust 单元测试：Profile、CIDR、路由表、Fake-IP/domain/IP 匹配、弱网决策；
2. Kotlin 单元测试：控制帧、VPN 授权、资源释放顺序、generation、TUN 超时关闭；
3. Infrastructure 测试：forward/reverse prepare/commit/rollback/uncertain、owner 崩溃恢复；
4. 模拟器门禁：安装 Companion、授权、ADB control、reverse、Listener、本地 upstream；
5. 真机门禁：目标 App 被接管、非目标 App 不受影响、ADB 保持可用、stop/fault 恢复网络；
6. 业务验收：真实 App、真实 Listener/TLS、真实 Server 和完整请求响应。

Listener running、VPN verified Running 和 ADB reverse 存在分别只证明一个局部事实。最终成功必须在
同一运行 epoch 中看到 App 请求、Proxy 转发、Server 回复、Proxy 返回 App 以及业务端结果。
