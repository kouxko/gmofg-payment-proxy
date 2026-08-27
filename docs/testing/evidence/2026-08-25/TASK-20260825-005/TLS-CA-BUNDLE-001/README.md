# TLS-CA-BUNDLE-001：上游多 CA PEM Bundle 测试资源

## 测试信息

- 任务 ID：`TASK-20260825-005`
- 用例 ID：`TLS-CA-BUNDLE-001`
- 状态：`PREPARED`
- 资源验证时间：2026-08-25 20:44:12 +08:00
- 资源验证：PASS
- Proxy 功能验证：NOT_RUN

## 目的

保存实现 Proxy 上游多 CA PEM Bundle 所需的原始证书、目标地址、OpenSSL 指令、资源基线和实现后
复测步骤，使后续实现会话不需要重新索要证书或猜测测试环境。

## 已提供资源

| 资源 | 原始来源 | 归档路径 | 用途 | 必需 |
| --- | --- | --- | --- | --- |
| `sub.pem` | 用户提供的 WeCom 文件缓存 | `resources/sub.pem` | 原始字节副本；First Data Latvia Intermediate CA | 是 |
| `DigiCertCA.pem` | 用户提供的 WeCom 文件缓存 | `resources/DigiCertCA.pem` | 原始字节副本；First Data Baltics Root CA | 是 |
| 后台地址 | 用户明确提供 | `inputs/backend.json` | Socket TLS 真实握手目标 | 是 |

原始文件路径仅记录来源；复测必须使用本目录归档副本：

```text
/Users/codin/WxWork/WXWork Files/Caches/Files/2026-08/ed68a42df6b1bd43a067828a55d30f19/sub.pem
/Users/codin/WxWork/WXWork Files/Caches/Files/2026-08/f42fc9da807d2b724e39ad9ea8bf1c68/DigiCertCA.pem
```

## 当前资源验证结果

- OpenSSL：`OpenSSL 3.6.3 9 Jun 2026`。
- Intermediate Subject：`DC=com, DC=1dc, DC=ne, CN=First Data Latvia Internal CA`。
- Intermediate Issuer：`CN=First Data Baltics root CA`。
- Root Subject/Issuer：`CN=First Data Baltics root CA`。
- `openssl verify -CAfile DigiCertCA.pem sub.pem`：PASS，输出 `sub.pem: OK`。
- 两个归档 PEM 与用户提供文件逐字节一致。
- 使用归档证书组合后的 Bundle 成员数量：`2`。

完整输出见 `outputs/resource-validation.txt`。

## 实现后复测

从仓库根目录执行 `steps/replay.md` 中的命令：

1. 使用 OpenSSL 将两张归档证书规范化并组合为 `FirstData-trust-chain.pem`。
2. 列出 Bundle 成员并确认数量和 Subject。
3. 验证 Intermediate 对 Root 的签发关系。
4. 执行实现后的 Rust 定向测试，证明解析、持久化和 Socket Trust Store 均加载两张证书。
5. 创建或更新 Socket Listener，目标为 `195.160.171.102:63002`，上游安全为 TLS，显式信任组合后的 Bundle。
6. 保存 TCP、TLS、证书链、hostname/IP、客户端证书请求和最终握手的分层结果。

## 成功标准

- Bundle 恰好包含归档的两张证书。
- Proxy 导入、重载和运行计划均得到两张证书。
- Socket TLS Trust Store 实际加载两张证书。
- 真实后台不再因缺少 Intermediate CA 产生证书链失败。
- hostname/IP 或客户端证书失败若存在，单独报告，不影响对 Bundle 加载能力的判断。

## 当前结果

- 测试资源：READY。
- 任务登记验证：PASS。
- OpenSSL 资源验证：PASS。
- Proxy 功能：NOT_RUN。
- 真实后台 Proxy 握手：NOT_RUN。
- 网络报文：N/A，本阶段未通过 Proxy 发送业务报文。
- UI 截图：N/A，本阶段未实现或运行 UI。
