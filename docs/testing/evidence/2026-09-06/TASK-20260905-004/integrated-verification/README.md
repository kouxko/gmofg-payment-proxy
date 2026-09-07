# HTTP/Socket 架构收敛集成验证

TASK-20260905-004，Asia/Shanghai。已实现独立范围通过验证和独立审查；Local HTTP 默认响应 Q1 待确认，任务尚未整体完成。

## 最终结果

| 验证 | 实际结果 |
|---|---|
| 全 workspace/all-targets/all-features cargo check | PASS |
| 最后补丁后的 Infrastructure lib | 479/479 PASS |
| 最后补丁后的 Runtime lib | 202/202 PASS |
| Frame Reject 精确回归 | RED: code=None；GREEN: 1/1，上下行字段保真 |
| write + flush 提交证明 | 7/7 PASS；包含真实字节及注入 flush 失败 |
| 全 workspace/all-targets/all-features strict Clippy | PASS |
| rustfmt / 架构扫描 / 脚本 lint / diff check | PASS |
| HTTP/Socket 架构守卫及破坏性变体 | 60/60 PASS |
| 独立 code-reviewer | APPROVE（已实现范围），P0/P1/P2 均为 0 |

完整全工作区运行得到 **1634 passed / 3 failed**。两项是旧 HTTP 错误字符串断言，已改为结构化 code 校验并在最终 Infrastructure 全套回归通过。另一项是未修改 MCP 的平台非 loopback HTTP 超时：同一用例单独重跑仍失败，但同接口基础 TCP 可连接，原因尚未确认。没有把全量测试记为全绿。

source-size 门禁仍报告 4 个未修改文件（546/516/516/504 行），HEAD 与当时工作区内容一致，见 [原有超限证据](outputs/existing-source-size.json)。此次触及的源码没有新增超限。完整编译输出中的平台 linker unwind 警告也保留于原始日志。

## 已验证行为与输入

- HTTP 固定入口与 Forward 共用 egress、Body 上限和取消/超时；原始 Header/非 UTF-8 bytes、1xx、chunked 与故障行为由全量运行中的 Runtime 集成测试覆盖。
- Writer 只有完整 write+flush 才产生 Committed；Body EOF 本身不提交；合法早响应走 ReadReady。精确输入与 PendingFlush/duplex fixture 在对应源码快照，真实输出见 write-commit-proof.log。
- Plain 同连接添加/删除 Document 规则：能力层与真实 TCP keep-alive 回归；生产 actor 的独立联合事务回归分别证明，不混称为同一端到端测试。
- Socket 规则、Encode、空输出/超限检查在 lifecycle commit 之前执行；失败 commit_attempts=0/hit_count=0，合法输出一次提交；Raw half-close 与协议配对保留。
- Frame Reject：输入为 `rejected frame`，拒绝原因刻意含 `Upstream|WRITE_FAILED` 误导文本；输出仍按明确 code/stage/direction 映射，观察阶段为 FrameInspect。

## 复测与资源

从仓库根目录执行：

```sh
python3 docs/testing/evidence/2026-09-06/TASK-20260905-004/integrated-verification/replay/verify.py
python3 docs/testing/evidence/2026-09-06/TASK-20260905-004/integrated-verification/replay/verify.py --post-review
```

第一条运行完整矩阵，保留每项失败并继续后续检查；第二条运行最后五文件补丁受影响回归和静态检查。脚本任一检查失败或测试期间源码变化都会非零退出。全量首次失败结果、第二次完整矩阵和最终局部复验分别保留，历史失败日志不会被文件名误标为通过。

[修改文件](outputs/changed-files.txt)、[完整资源索引](outputs/resource-index.json)、[最终五文件快照索引](outputs/post-review-resource-index.json)、[最终逐字节比较](outputs/post-review-resource-comparison.json)提供源路径、实际快照和用途。资源保留内联报文、预期字段、fixtures、脚本与 Cargo/Deno 版本输入；来源仍按仓库相对路径保存。两轮记录的测试前后 size/mtime 状态均一致；没有测试期间源码修改。

[完整执行结果](outputs/results.json)、[最终复验结果](outputs/post-review-results.json)、[最终独立审查](outputs/review-final.md)和 metadata.json 为本例权威结果。初次审查和集中复核 REQUEST CHANGES 也保留于 outputs。

UI/截图、真实设备/外部支付业务、远程 CI/发布：N/A，未执行、不声称验收。Local HTTP 默认状态/响应头不在已实现范围，见 [当时待确认任务（现已归档）](../../../../../tasks/completed/2026-09-06/unify-http-socket-runtime-contracts.md)。
