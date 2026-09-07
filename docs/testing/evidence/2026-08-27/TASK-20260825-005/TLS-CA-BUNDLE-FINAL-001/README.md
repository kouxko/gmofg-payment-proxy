# TLS-CA-BUNDLE-FINAL-001：上游多 CA PEM Bundle 最终验收

## 结果

- 状态：PASS
- 执行时间：2026-08-27 15:12:00 +08:00
- 目标：证明一个 PEM 文件中的全部 CA 会被解析、规范化、受保护持久化、重启恢复并进入 Socket TLS Trust Store。

## 输入与环境

- 原始资源沿用 `2026-08-25/TASK-20260825-005/TLS-CA-BUNDLE-001/resources/`。
- Bundle：`2026-08-25/TASK-20260825-005/TLS-CA-BUNDLE-001/outputs/FirstData-trust-chain.pem`。
- 真实目标：`195.160.171.102:63002`，TCP + TLS。
- OpenSSL：3.6.3。
- 业务报文：N/A；本用例只执行证书解析和 TLS 握手，不发送业务字节。
- UI 截图：N/A；UI 文案由自动化组件测试验证。

## 实际验证

1. 两张 CA 按输入顺序解析，规范化后再次解析仍得到相同两个成员。
2. 任一成员不是 CA 时，整个 Bundle 被拒绝，不返回部分成功。
3. Bundle 经现有 Listener 导入入口保存；重新创建 Adapter 后解析得到两个成员。
4. 本地 TLS Server 只发送叶子证书：只给 Root 的探测失败，给 Intermediate + Root Bundle 的探测成功。
5. First Data 真实目标使用归档 Bundle 完成 TLS 1.3 握手，证书链校验为 OK。
6. 单证书、客户端身份、下游 TLS/mTLS 和现有 Socket TLS 回归保持通过。

详细结果见 `outputs/local-validation.txt` 与 `outputs/remote-tls.txt`；复测命令见 `replay/replay.md`。
