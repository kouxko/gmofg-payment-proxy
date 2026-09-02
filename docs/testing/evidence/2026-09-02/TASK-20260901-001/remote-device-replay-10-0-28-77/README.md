# Remote Windows Wasm and rules replay on 10.0.28.77

## Result

`PASS`：远端 Windows App 已真实加载 5 个仓库 Wasm Component。最终 Workspace 的 5 条 Socket
Listener 均保持运行且 `fault_reason=null`；5 条 `proxy_to_upstream` 规则分别命中 1 次，受控上游和
客户端的实际字节符合预期。4 条未命中用例保持原字节且未增加规则命中计数；4 条非法 Frame 在连接
上游前 fail-closed。HTTP 未重跑。

| Package | Listener -> upstream | Hit request/response | Rule result |
| --- | --- | --- | --- |
| `au-eftex@1.1.0` | `0.0.0.0:8083` -> `10.0.34.59:18086` | 71/63 bytes | `message_type.value=1200`，命中 1 次，公开测试 BDK |
| `iso8583-ascii-standard@1.0.0` | `0.0.0.0:8084` -> `10.0.34.59:18084` | 6/6 bytes | MTI `0200 -> 0100`，命中 1 次 |
| `iso8583-deno-ascii@1.0.1` | `0.0.0.0:8085` -> `10.0.34.59:18085` | 59/59 bytes | MTI `0200 -> 0100`，命中 1 次 |
| `nuvei-tango-json@1.0.1` | `0.0.0.0:8086` -> `10.0.34.59:18087` | 48/49 bytes | 同值规则命中 1 次；双向递归嵌套 Display |
| `nuvei-tango-json-rhai@1.0.1` | `0.0.0.0:8087` -> `10.0.34.59:18088` | 271/191 bytes | 同值规则命中 1 次；双向递归嵌套 Display |

同值规则用于 AU EFTEX 和两套 Nuvei，只观测规则执行而不修改受 MAC/只读 Encode 合同保护的字段；
三条规则的持久化 `hit_count` 均从 0 变为 1，因此不是仅靠字节不变推断命中。

## Rule replay evidence

命中 Exchange：

- AU EFTEX：`0ac5adbbec534b03bce36567d99889b2`
- ISO standard：`c4a55bd93ae248d8b23dc37bab15d77b`
- ISO Deno：`60763396376c441dad64b1cd29b83ee2`
- Nuvei JSON：`f70011179c924d2183cfa9c12f110156`
- Nuvei Rhai：`079b3f72d24a49eb906516205bd9452b`

未命中 Exchange：ISO standard `4d241a79938b43b3bdfcd760dbfa9e45`、ISO Deno
`2d950b9a674d4d6aaba9103380d6c2d8`、Nuvei JSON `e0a5e0a190a34d639d305f6301d33488`、
Nuvei Rhai `78674144b1f44307b433de8d45a88774`。四条均 `completed`、逐字节保持，所有规则
`hit_count` 仍为 1。AU miss 因只有一组权威加密上行向量而 `NOT_RUN`。

非法 Frame Exchange：ISO standard `0f9dcefdddd74c8f909bb481b270a131`、ISO Deno
`98459d41672e4caba5087e4028fe8838`、Nuvei JSON `8fdeeba0e3ea4cf8975db429241f7d04`、
Nuvei Rhai `25e6322f34d64b73b5954bda6296ea1f`。前两条为 `DECODE_FAILED`，后两条为
`PROCESSING_FAILED`，受控上游均收到 0 bytes。AU 非法密文向量 `NOT_RUN`。

Exchange store 最终共 13 条：9 条 completed，4 条预期 fail-closed，0 dropped、0 evicted。
Diagnostics cursor 为 348；其中 4 个 error 恰好对应上述 4 个预期非法 Frame，没有 Listener fault。

## Display regression evidence

- ISO Deno 1.0.1 接受 Host 规范化产生的有限、非负、数学整数 JSON number，仍拒绝负数和小数；
  正式 Wasmtime Host 与远端双向 Display/Encode 均通过。
- Nuvei JSON 1.0.1 从脱敏后的 `json_preview.value` 解析对象和数组，递归输出
  `<table class="protocol-document-nested">`；标量继续 HTML escape。
- Nuvei Rhai 1.0.1 同样递归渲染 object/array。两套 Nuvei 的命中、未命中、上下行 Display 均包含
  nested table 且不包含 `<pre>`。

## Configuration evidence

导入新 Wasm 后旧 Workspace 不再存在。MCP 首次 new-target 候选因受控上游尚未启动，在
`dns_tcp_port` 层返回 `validation_failed`，没有状态变更；上游启动后重新创建并提交：

- Workspace：`Remote Wasm Rules Replay 20260902`
- Workspace ID：`5578cfdb-4b94-4d7d-be99-fec767329700`
- Candidate ID：`f26995c0-74df-4239-9be8-df265d56d341`
- Apply task ID：`1b4b11a9-1d51-4078-b3f2-58d8fc428772`
- 初始 revision 1；规则测试后 revision 11
- Runtime epoch：`2db8a6c6-4f14-4567-9164-3d560ac7a146`

用户启动后 5 条 Listener 均为 running。受控上游实际看到来源地址 `10.0.28.99`；MCP 与客户端按
用户提供的 `10.0.28.77` 访问。精确 Listener、规则和结果见 `outputs/rule-remote-state.json` 与
`outputs/rule-results.json`。

## Resources and replay

`resources/` 保存远端实际导入的 5 个 Wasm 和 Nuvei 原始 fixture；`inputs/vectors.json` 保存初始
Display 回归向量，`inputs/rule-candidate.json` 保存最终配置合同；`replay/rule_harness.py` 与
`replay/rule_client.py` 是本次实际运行入口。完整步骤见 `steps/replay.md`。

## Not applicable

- TLS/mTLS：本用例固定 TCP -> TCP transparent。
- HTTP：本次范围是 5 个 Socket Wasm 与规则；历史 HTTP 用例未重跑。
- AU miss/非法密文：没有第二组权威加密向量，未伪造密文。
- CI、发布、生产密钥：未执行；AU Component 只包含公开 ANSI 测试 BDK。
