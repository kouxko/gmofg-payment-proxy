# PAYMENT-DLL-D48-MOCK-001

- 任务：TASK-20260903-001
- 执行时间：2026-09-03 12:25:24 ～ 12:36:09 +08:00
- 环境：macOS；运行中的 `/Applications/Intercept Proxy.app`；MCP `127.0.0.1:17653/mcp`
- 结果：`PASS_WITH_MCP_FULL_CANDIDATE_INCOMPATIBILITY`

## 目的与前置条件

从现有 Payment DLL HTTP 抓包生成启用的 D48 Mock，保留固定 Root、Listener、Android 路由、上游
mTLS 身份引用和已有规则，并用无真实上游的本机请求验证命中。

## 步骤与实际结果

1. MCP `application_snapshot` 固定 Workspace revision 17、Listener、证书和原规则基线。
2. MCP `environment_candidate_create` 的 schema 层通过，但 domain 层返回
   `LISTENER_DOMAIN_INVALID`。原因是当前合法 UI 配置允许 TLS Listener 在
   `server_identity=null` 时使用固定 Root 动态签发，而环境候选 domain 要求 TLS 开启时必须提供
   `server_identity_alias`；未 apply，因而没有修改 Workspace。
3. 为避免关闭 TLS、替换证书或丢失现有上游身份引用，改用当前 App 的规则编辑入口保存完全相同的
   Mock 定义。UI 明确提示规则已保存。
4. MCP 回读 Workspace revision 18、规则 ID、完整 MockResponse、固定 Root 指纹和上游客户端身份引用；
   基线配置均保留。
5. 短暂启动 Listener，向本机不可用上游 `127.0.0.1:9` 发送请求。返回日志中的 HTTP 200/D48 正文；
   MCP 抓包显示 `Mock 响应`，目标规则命中 1 次，终态耗时 3 ms。随后停止 Listener，恢复原状态。

## 逐字段比较

- HTTP status：预期 200，实际 200，PASS。
- Header：`cache-control`、`content-type`、`date` 一致；`Content-Length` 由运行时生成 118，PASS。
- Body：预期与实际均为 118 字节 D48 JSON，PASS。
- 规则：request-stage terminal Mock，`RequestTarget Equals /`，启用，priority 100，PASS。
- 固定 Root 指纹：前后均为 `B4:72:77:A5:8D:81:AD:EB:3C:CE:59:7A:15:58:85:4D:AB:3D:0B:30:AB:CE:15:06:5A:FB:73:33:9B:CB:D7:4C`，PASS。
- 上游客户端身份引用：前后均为 `8f6c650a-5fb9-4ad8-98b6-f5acdc6ed9de`，PASS。
- 真机 Payment App 再发交易：NOT_RUN；本次没有授权或需要真实业务交易。
- CI：N/A；没有源码变更。

## 资源与输出

- 原抓包输入：`inputs/source-http-exchange.json`
- 失败的完整 MCP 候选：`inputs/environment-candidate.json`
- MCP 候选结果：`outputs/environment-candidate-create.json`
- 本机响应：`outputs/local-replay-response.http`
- MCP 回读：`outputs/mcp-readback.json`
- 复测入口：`replay/README.md`
