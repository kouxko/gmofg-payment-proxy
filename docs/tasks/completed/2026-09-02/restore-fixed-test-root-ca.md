# 恢复跨平台固定测试 Root CA

## 任务信息

- 任务 ID：`TASK-20260902-003`
- 状态：`已完成`
- 任务日期：`2026-09-02`
- 创建时间：`2026-09-02 18:22:16 +08:00`
- 开始时间：`2026-09-02 18:22:16 +08:00`
- 最后更新时间：`2026-09-02 18:29:44 +08:00`
- 完成时间：`2026-09-02 18:29:44 +08:00`
- 创建路径：`docs/tasks/pending/2026-09-02/restore-fixed-test-root-ca.md`
- 归档路径：`docs/tasks/completed/2026-09-02/restore-fixed-test-root-ca.md`
- 关键词：`Root CA`、`固定测试证书`、`Android Payment`、`TLS`、`证书指纹`
- 任务优先级：`高`
- 优先级理由：变更下游 TLS 信任根和随包签发私钥策略，直接影响 Android Payment 到 Proxy 的握手合同。

## 背景与目标

`2026-08-06` 的提交 `a5b403d` 将 macOS 与 Windows 受控测试 Proxy 统一为固定 Root CA，公开指纹为
`B4:72:77:A5:8D:81:AD:EB:3C:CE:59:7A:15:58:85:4D:AB:3D:0B:30:AB:CE:15:06:5A:FB:73:33:9B:CB:D7:4C`。
Android Payment `server.crt` 及 A920MAX 当前 APK 均信任该指纹。

提交 `50288dd` 把 `InterceptProxyProfile` 的固定 Root 返回值改为 `None`，改成每安装实例生成独立 Root，
并把上述固定 Root 标记为撤销。A920MAX 当前请求已到达远端 Proxy，但在 App 校验下游证书链时报告
`CertPathValidatorException: Trust anchor for certification path not found`。

用户现已明确要求恢复原合同：Proxy 继续使用固定且不变化的测试 Root CA，与 Payment APK 内置
`server.crt` 保持一致。

## 范围

- `InterceptProxyProfile` 重新提供仓库内固定 Root CA 及配套 PKCS#8 私钥。
- 删除针对该固定 Root 的撤销识别和启动阻断。
- 保持叶子证书按当前 SAN/SNI 动态签发；只固定签发根，不固定所有叶子证书。
- 更新产品策略、证书启动同步和文档测试，固定验证公开 SHA-256 指纹。
- 保存可重复的证书与定向测试证据。

## 不在范围

- 不修改 `/Users/codin/Code/jp_gmofg_payment` 的 `server.crt` 或 Payment TLS 代码。
- 不改变上游 Server Trust、上游 PKCS#12 客户端身份或 mTLS 业务配置。
- 不重置或删除任何本机、远端 Proxy 或 Android 设备数据。
- 不触发远程 CI、发布、上传、推送或真实支付交易。

## 需求确认记录

| 时间 | 结论 |
| --- | --- |
| `2026-09-02 18:22:16 +08:00` | 用户明确否定每安装实例生成 Root，要求修改并继续使用固定证书。 |
| `2026-09-02 18:22:16 +08:00` | 对话上下文与 A920MAX 已安装 APK共同确认固定证书为指纹 `B4:72:...:D7:4C` 的 `Intercept Proxy TEST ONLY Root CA`。 |
| `2026-09-02 18:22:16 +08:00` | 固定 Root 仅用于当前受控测试链路；本任务不扩大到生产或真实商户信任体系。 |

## 未确认事项

无。固定 Root 文件、指纹、Payment 消费方式和 Proxy 目标行为均已有当前源码、APK与用户确认。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`，恢复固定 Root，使不同 Proxy 安装与 Payment APK共享同一信任锚。
- 范围与不在范围：`PASS`，只修改 Proxy 固定测试 Root 策略及其测试/文档。
- 输入、输出和状态：`PASS`，输入为仓库固定证书/私钥；输出为相同 Root 下签发的运行叶子证书。
- 错误行为：`PASS`，固定证书或私钥无效时继续 fail-closed；不增加回退或自动迁移。
- 具体示例：`PASS`，Payment APK 信任指纹 `B4:72:...:D7:4C`，Proxy 初始化后导出同一指纹。
- 可重复验收：`PASS`，OpenSSL 指纹、产品策略单测、证书同步/运行材料测试可直接判定。
- 会改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-09-02 18:22:16 +08:00`

## 问题与根因分析

- 实际现象：A920MAX 经 ADB reverse 可连接远端 Proxy 8080，但 Payment TLS 报 `Trust anchor for certification path not found`。
- 预期行为：所有受控测试 Proxy 使用与 Payment `assets/server.crt` 相同的固定 Root，App 可验证 Proxy 叶子链。
- 最小复现：启动 Android VPN 路由后，由 `jp.gmofg.app` 请求 `https://https.gmo-fg.net:16127/`。
- 当前已验证：设备网络与 reverse TCP 连接成功；Payment APK内置 Root 指纹为 `B4:72:...:D7:4C`；当前 HEAD 产品策略返回 `None`。
- 推断：当远端 Proxy 使用 `50288dd` 之后的独立 Root 时，Payment 自定义 TrustManager 无法找到其信任锚。
- 未知：`10.0.28.99` 当前运行二进制的精确 commit 与持久化 Root 指纹尚未读取。
- 候选原因：网络/端口已排除；TLS 版本或 hostname 与当前异常类型不符；Root 策略漂移与源码和异常一致。
- 已确认根因：提交 `50288dd` 改变了已约定的固定 Root 产品合同，Proxy HEAD 与 Payment APK信任锚不再天然一致。
- 影响范围：新初始化或已重置的 Proxy 安装、证书导出、动态叶子签发及所有内置旧固定 Root 的测试客户端。

## 最小改动与最优设计比较

| 方案 | 分析 |
| --- | --- |
| 只把 Payment 的 `server.crt` 换成某台 Proxy 的 Root | 会把固定客户端变成逐实例维护，违背用户确认，不采用。 |
| 保留实例 Root 并同时信任旧固定 Root | 引入双路径、兼容回退和不明确的实际签发根，不采用。 |
| 恢复产品策略显式固定 Root，并删除相反的撤销阻断 | 复用已有证书装载、签发和同步路径，单一合同、改动最小，采用。 |

## 小任务与验收

| ID | 任务 | 状态 | 验收 |
| --- | --- | --- | --- |
| FRC-01 | 恢复固定 Root 产品策略 | 已完成 | 产品 Profile 返回固定证书和私钥，指纹与 Payment 一致 |
| FRC-02 | 删除相反的撤销阻断 | 已完成 | 固定 Root 可完成启动同步和运行材料冻结 |
| FRC-03 | 同步文档与 UI 文案 | 已完成 | 权威文档不再声明每安装实例独立 Root或要求删除固定 Root |
| FRC-04 | 回归与证据 | 已完成 | 定向 Rust 测试、OpenSSL 指纹和静态检查通过 |
| FRC-05 | 对抗审查与归档 | 已完成 | 用户明确要求跳过对抗审查；任务和证据索引已归档并校验 |

## 测试计划

- OpenSSL：验证固定 Root 自签名、CA/Key Usage、有效期和 SHA-256 指纹。
- `intercept-proxy-product-api`：固定 Root/私钥存在且证书指纹稳定。
- `intercept-proxy-infrastructure`：空存储初始化固定 Root、现有相同 Root复用、不同 Root fail-closed。
- 证书运行材料：固定 Root可签发当前 SAN/SNI 叶子并通过链验证。
- `cargo fmt --check`、受影响 crate Clippy/测试、`git diff --check`。
- A920MAX 真实业务握手：本任务不自动发送交易；在远端新构建部署后另行验证并明确记录。

## 文档影响

- `src-tauri/resources/certificates/README.md`
- `src/features/certificates/certificates-view.tsx`
- `src/features/help/system-page-help-guides.ts`
- 任务与测试证据索引

## 对抗审查计划

- `NOT_RUN`：用户在 `2026-09-02` 最新明确要求“不要对抗审查了 测试完提交”，因此停止已启动的只读审查，不以未完成审查作为验收证据。

## 实施记录

- `2026-09-02 18:22:16 +08:00`：完成当前设备、Payment APK、Proxy HEAD 和 Git 历史只读核对；确认固定 Root 合同被 `50288dd` 改为实例 Root。
- `2026-09-02 18:26:00 +08:00`：恢复产品 Profile 固定 Root/私钥，删除固定 Root 撤销阻断，保持非当前持久化 Root fail-closed。
- `2026-09-02 18:27:00 +08:00`：同步基础设施回归、证书页面及帮助文案，明确跨安装 Root 固定、叶子证书动态生成。
- `2026-09-02 18:29:44 +08:00`：完成 Rust、前端、OpenSSL、逐字节证书比较和静态检查并归档证据。

## 修改文件

- `src-tauri/crates/product-api/src/intercept_profile.rs` 与证书策略测试。
- `src-tauri/crates/infrastructure/src/adapters/certificates/`、证书同步测试及应用层说明。
- `src-tauri/resources/certificates/README.md`。
- `src/features/certificates/`、`src/features/help/system-page-help-guides.ts`、`src/features/listeners/downstream-tls-card.tsx`。
- 任务、完成索引与测试证据索引。

## 附加文件

- [固定测试 Root CA 回归证据](../../../testing/evidence/2026-09-02/TASK-20260902-003/fixed-test-root-regression/README.md)

## 验收结果

- `PASS_WITH_REMOTE_HANDSHAKE_NOT_RUN`。
- Proxy 与 Payment 使用完全相同的固定 Root PEM，SHA-256 指纹为 `B4:72:77:A5:8D:81:AD:EB:3C:CE:59:7A:15:58:85:4D:AB:3D:0B:30:AB:CE:15:06:5A:FB:73:33:9B:CB:D7:4C`。
- 不同 Proxy 安装共享固定 Root/私钥，但叶子证书与 SAN 按各自监听地址独立生成。
- 远端 `10.0.28.99` 未部署本次构建，真实 A920MAX 握手未验证。

## 测试结果

- `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`：PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-product-api`：6 passed。
- `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure certificates_tests:: -- --nocapture`：39 passed。
- `deno run -A --unstable-detect-cjs node_modules/vitest/vitest.mjs run src/features/certificates/certificates-view.test.tsx`：8 passed。
- OpenSSL 指纹与 Payment `server.crt` 逐字节比较：PASS。
- `git diff --check`：PASS。

## CI 情况

`NOT_RUN`：未授权触发远程 CI。

## 完成总结

已恢复固定测试 Root CA 产品合同并完成本地回归。当前提交证明源码、证书和定向自动化一致；远端部署、持久化证书状态处理及 A920MAX 真实握手仍需在部署阶段单独验证。
