# 修复 Android VPN 下 Launcher 本地 TCP 不可达

- 任务 ID：TASK-20260908-002
- 状态：进行中
- 优先级：高（VPN 安全边界、生命周期及真实设备验证）
- 任务日期：2026-09-08
- 创建时间：2026-09-08 16:29:35 +08:00
- 开始时间：2026-09-08 16:29:35 +08:00
- 最后更新时间：2026-09-08 16:49:56 +08:00
- 完成时间：未完成
- 创建路径：docs/tasks/pending/2026-09-08/fix-android-vpn-local-tcp.md
- 归档路径：docs/tasks/completed/2026-09-08/fix-android-vpn-local-tcp.md
- 关键词：Android、VpnService、allowBypass、IIF、PAX、7788

## 背景、目标与范围

用户确认在 develop 新建 hotfix 修复 Companion VPN 导致 Launcher TCP 7788 不可达，并覆盖安装到远程设备 10.0.34.117:5555 在线验证。基线为 gitlab/develop 6764210eb057bb482b86a05cd1df23d7cdea5c5c，分支 hotfix/fix-android-vpn-local-tcp；隔离 worktree 避免混入原工作区的其他 hotfix。
范围：VpnTunFactory 在 establish 前调用 allowBypass；签名 release APK、保留数据覆盖安装、重建原 VPN、TCP 握手及安全的默认出站路径检查。
不在范围：Launcher/PAX SDK、业务报文、交易、额外配置/UI/兼容回退、推送、CI、发布。

## 需求确认与就绪

用户先询问方案，已说明 allowBypass 作用于整个 VPN：默认出站仍路由进 TUN，但显式绑定底层 Network 的应用可绕过；不是仅开放 7788。用户 2026-09-08 16:23:58 +08:00 确认修复及覆盖安装。
输入：原 Profile、四个已选包、原设备与 TCP 模式。输出：VPN 活动期间 7788 握手成功；保留默认出站接管。错误继续遵循现有 establish 失败传播，无新兜底。
示例：Mac 10.0.34.43 到 10.0.34.117:7788 零应用数据握手，从超时变为连接成功。
就绪检查：目标、边界、状态变化、错误、示例、可判定验收已明确；未确认事项无；2026-09-08 16:29:35 +08:00 进入实现。

## 现象、证据与根因

原快速验证 [QV-20260908-001](../../../testing/quick-validations/2026-09-08/QV-20260908-001/README.md) 在 Launcher TCP 模式、uid10116 监听 *:7788、VPN tun0 活动时，8 秒零载荷连接超时。复用其输入、设备系统 netd.o 与反汇编；该档案保持不可变，本任务重新验收。
当前已验证：设备 Android 10 PAX 固件 PermissionMonitor 将四个目标 UID 绑定 tun0；实际 netd.o ingress 程序检查 IIF_MATCH，非允许入接口丢弃。设备 services.jar 的 requiresVpnIsolation 在 allowBypass=true 时返回 false；Vpn 将配置值传入 NetworkMisc，且 allowBypass 变化不可原位切换。always_on_vpn_lockdown/app 均 null。
候选原因：USB 模式（用户切 TCP 后仍失败，排除）；未监听（确认 LISTEN，排除）；单纯 SYN-ACK 回程（fwmark 路由存在，不能解释 ingress IIF，降级）；VPN UID 入接口隔离（设备系统代码、活动规则与行为一致，确定修复边界）。
根因链：Companion 未 allowBypass → 系统对有默认路由的 VPN 安装 UID IIF 限制 → WLAN 来包无法交付 Launcher TCP server。单包 BPF drop 计数因无 root/抓包权限未知；最终因果以同设备改前改后对照确认，不把静态推断当单包证明。
影响：包含目标 UID 的 LAN 入站，安全边界放宽为可显式 bypass；不是所有 Android 10 固件的通用断言。

## 最小改动与最优设计

最小：VpnTunFactory 增加 allowBypass 和意图注释，沿用原 Profile/路由/错误/资源所有权。最优：由 VPN 建立边界表达允许底层网络访问，不修改业务 Socket 或拆 UID；本场景两者一致。无依赖、无持久字段、无并行实现。

## 小任务与验证计划

1. 主 Agent：登记、实现、文档（顺序）；验收唯一 Builder 路径正确调用。
2. 主 Agent：lint、现有单元、四 ABI release 构建及 signer/对齐验证；保存原始输出与被测快照。
3. 主 Agent：原 APK/设备状态备份、install -r、重建 VPN、7788 三次零载荷握手、默认路由/可用安全出站验证。交易不发送，无法执行层级如实 NOT_RUN。
4. 独立 Agent：全部结果集成后整体对抗审查；APPROVE 后日期归档和索引检查。
小任务审查合并为任务级独立审查，避免重复；高优先级整体门禁保留。

## 文档影响

android-companion/README.md 与架构 Android VPN 文档需要更新安全与重建说明。根 README、用户 UI、数据库、协议包、MCP、其他架构无需修改（接口与操作入口不变）；任务/证据索引需要同步。

## 实施、测试与验收

进行中；证据待补充。使用已读取 android-cli Skill 进行既有 CLI 设备交互；项目登记、权限、资源归档与审查门禁保留。
CI：NOT_RUN（未授权且未触发）。业务交易：NOT_RUN（不在范围）。

### 2026-09-08 构建与安装进展

- 产品改动已完成：VpnTunFactory.kt 两行；Companion README 与 Android VPN 架构说明同步。无新依赖、配置字段或兜底。
- [local-build](../../../testing/evidence/2026-09-08/TASK-20260908-002/local-build/README.md)：45/45 单元、Lint零错误/5项非本改动位置警告、四ABI release、固定签名/16KiB门禁、实际DEX复核PASS。
- [device-vpn-tcp](../../../testing/evidence/2026-09-08/TASK-20260908-002/device-vpn-tcp/README.md)：原APK备份；install-r成功；安装hash与新产物逐字节hash一致；UID/firstInstallTime保持；安装前后VPN停止状态各三次零载荷7788握手成功。
- 当前未完成：原控制电脑的VPN已因租约失联停止，需恢复原配置后验证活动VPN/IIF/默认出站。已通知用户，本机不能读取或重建远端桌面私有Profile/路由。
- 当前保持进行中，不归档为已完成。未执行交易、推送、CI或发布。局部静态审查与最终任务级审查分开，最终APPROVE待全部在线证据集成后执行。

### 独立局部审查

code-reviewer只读审查18文件，当前实现/构建/覆盖安装范围APPROVE，P0/P1/P2=0，见 [审查记录](../../../testing/evidence/2026-09-08/TASK-20260908-002/local-build/outputs/implementation-review.md)。未把局部APPROVE作为任务完成；在线门禁仍待原控制电脑恢复配置。当前已验证实现与证据可独立本地提交；最终任务级审查待在线结果集成后执行。
