# Intercept Proxy 验证与排障指南 v1.0

本文供 MCP 客户端在配置、测试和排障时使用。它描述验证顺序和停止条件，不代表当前 Listener、设备、
远端服务或证书一定可用。需要当前事实时，应调用对应只读工具重新查询。

## 1. 回答规则

先把信息分成三类：

- **已观测事实**：当前工具结果、同一 Exchange 的事件、当前 runtime epoch 或本次实际握手结果。
- **推断**：由事实支持但尚未直接证明的原因，必须说明还需哪一步验证。
- **未知**：缺少设备、远端、证书、报文或运行环境，不能用历史 PASS 代替。

遇到失败时保持原错误码和目标身份。不要建议关闭证书校验、忽略错误、把 Scripted 降级成 Direct、
伪造空响应或重写受 MAC/签名保护的报文。任何步骤失败都先停在该层，不继续声称后续业务成功。

## 2. 通用分层顺序

1. **配置**：确认唯一 Workspace、Listener、模式、地址、端口、协议包精确版本和材料引用。
2. **生命周期**：确认 Listener 状态、runtime epoch、候选或 Android owner 没有 stale/CAS 冲突。
3. **网络**：分别验证 DNS、TCP connect、监听地址和目标地址；连接成功不等于协议成功。
4. **安全**：验证 TLS 版本、CA 链、SNI/hostname、客户端身份和服务端最终认证结果。
5. **Framing**：确认一次消息的长度边界、分段、粘包、EOF 和剩余字节处理。
6. **协议**：确认 Decode、Display、Rules、Encode 的方向、schema、版本和预算。
7. **业务报文**：比较 Proxy 实际收到、实际发送、Server 实际收到和 App 最终收到的内容。
8. **观测闭环**：用同一 exchange_id/runtime epoch 对齐 Received、Sent、Failed、Closed 和诊断日志。

TCP 成功但 TLS 失败时不要继续分析业务字段；TLS 成功但没有完整 Frame 时不要归因于规则；规则测试
只有在实际 Sent 内容与预期一致时才算完成。

## 3. HTTP

- Fixed Server 验证 method、完整 request-target、headers、body 和响应。
- Forward Proxy 验证 absolute-form authority；CONNECT/Upgrade 当前是明确不支持路径，不能当成功隧道。
- 修改 Body 后检查实际 `Content-Length`；不要保留 `Connection`、`Proxy-Connection`、
  `Transfer-Encoding`、`Upgrade` 等 hop-by-hop Header，`Connection` 点名的扩展 Header 也必须移除。
- 从抓包响应生成 Mock 草稿时，只选择 Server → Proxy 的完整 HTTP 响应。压缩、二进制、非 UTF-8、
  证据淘汰或缺少配对请求时停止；草稿默认禁用且未保存，人工检查后再保存。
- 这类拒绝使用 `HTTP_MOCK_DRAFT_*` 稳定错误码；保留原码，不把失败替换成空草稿。
- 对 JSON、XML 或文本的规则判断必须以 Listener 实际处理模式为准。未配置协议包时不要假定存在
  Document，也不要把显示文本当作可逆原始字节。

## 4. Socket、Frame 与协议包

- Direct 只证明透明 transport；Scripted 才执行 Frame → Decode → Display → Rules → Encode。
- 分段输入应返回 NeedMore；完整一帧立即处理；粘包只能消费当前帧并保留余量。
- 无修改时比较原始 wire bytes；JSON 或 Document 重新序列化即使语义相同也可能改变 MAC/签名字节。
- 协议包必须使用 manifest 中的精确名称和版本。确认方向、schema、stage 以及外部包 generation/RPC ID。
- Display 失败只影响观测；Frame/Decode/Rules/Encode/hook 失败必须终止当前 Exchange，不能透明转发。

## 5. TLS 与 mTLS

按以下顺序报告：TCP → TLS 版本/套件 → server trust → SNI/hostname → client identity → 零业务字节握手
结果 → 应用报文。常见判断：

- 错 CA：先确认引用角色是 upstream server trust，而不是客户端身份或 downstream server identity。
- hostname 失败：确认连接 Host 与 SNI 是否独立配置；要求 hostname 校验时，没有可验证 DNS 名必须
  fail closed。
- mTLS 缺失/错误客户端证书：客户端写出 ClientHello 不代表最终成功，必须等待服务端认证结果。
- 握手负例必须确认未发送应用业务字节。

不要建议 `verify_hostname=false`、信任任意证书或复用私钥作为快速修复。

## 6. Android 多设备

- 所有设备操作使用显式 serial；apply/stop/emergency 同时携带 expected runtime epoch。
- owner 列表是按 serial 排序的权威集合，selected device 只用于界面选择，不能作为执行回退。
- 错误必须保留目标 serial 和 SQLite 权威 epoch；权限错误不能降级成“设备离线”。
- A 设备失败不能阻止 B 的 reconciliation/shutdown 尝试；迟到的 epoch1 响应不能清除 epoch2 状态。
- package UID/shared UID、profile draft、ADB forward 和端点查询都按 serial 隔离。

## 7. Environment candidate

1. 读取 capabilities，确认传输和预算。
2. create 后检查七层报告和公开 preview；create 返回前断开应取消并清理私有材料。
3. status 确认仍是 preview_ready；baseline、配置或 runtime 改变后重新创建。
4. apply 只接受同一候选的一次性 token；成功响应仅表示 apply_queued。
5. apply 后断开不能取消 Application 已接管的工作；用 status 观察 terminal result。

终态只接受已注册稳定码；non-committed 结果不得包含伪造持久化 ID。

## 8. 建议使用的 MCP 证据

- `application_snapshot`：先确认 Workspace、Listener 和 generation 一致。
- `listener_status`、`android_network_status`、`android_runtime_owner_list`：确认运行所有权和 epoch。
- `exchange_observation_query` / `exchange_observation_get`：对齐真实四方向报文与失败阶段。
- `diagnostics_query`、`application_log_query/get`：补充控制面错误，不用它们替代报文证据。
- `protocol_package_detail`、`external_package_service_status`：确认精确包版本、在线状态和 hook 能力。
- `reproduction_report`：汇总配置与日志；它不包含完整 Exchange payload。

## 9. 停止与结论

- **PASS**：本次范围的必要层均有当前输入输出或状态转换证据。
- **FAIL**：实现或结果违反合同；保留首个失败层、错误码、目标身份和重放步骤。
- **NOT_RUN**：真实设备、远端、证书或人工 UI 不可用；列出缺失条件和复测入口，不用其他测试替代。

只在证据能直接支持时说“成功”。历史用例适合复用步骤和资源，不应作为当前环境结论。
