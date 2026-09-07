# 已实现批次独立架构审查

- 任务：TASK-20260905-001；审查者：`/root/final_architecture_review`，独立 architect，未参与实现。
- 时间：2026-09-05 15:07:49 +08:00
- 进入条件：全部实现分片已终态、结果收集/集成完成，Rust1120、最终前端构建、fmt和architecture检查通过后启动。
- 结论：`APPROVE`（仅 #1/#2/#4/#5/#6/#7/#8/#12 已实现范围）。当前确认的新 P0/P1/in-scope P2：0。
- 方法：只读完整任务source diff、新回归及调用链/锁序，核对父任务保护合同；不把测试退出码替代架构分析。

## 核对结果

1. Android native故障只有当前generation可进入原子faulted+teardown，replacement start在TUN清理完成前不能穿透。Native stop在取走handle后释放Rust全局mutex再等待，未确认Kotlin registry锁与Rust DATA_PLANE锁的反向等待环。依据：`VpnRuntimeRegistry.kt:173`、`NativeDataPlaneFailureCoordinatorTest.kt:48`、`jni_bridge.rs:50`、`data_plane.rs:174`。
2. heartbeat仅使用owner/runtime快照且要求Active/Uncertain和匹配epoch；不提交持久业务owner/reverse状态。临时forward仍由本次协议请求持有，精确serial+port清理；同serial业务gate保持。依据：`android_adb/lease.rs:85`、`control_port.rs:147`、`bundle.rs:295`、`tests/lease_heartbeat.rs:80`。
3. Bootstrap在首个异步读取前取cursor，并有暂停读取期间事件回放回归。请求清理所有Connection动态token和Proxy-Connection，普通raw头表示保持；chunked/404和故障语义回归未回退。依据：`facade/lifecycle.rs:11`、`requirements_tests/events.rs:241`、`proxy/src/message.rs:314`。
4. Listener以base/latest/draft逐项合并，同项冲突保留草稿并阻断保存/启动/删除；显式discard只处理当前Listener，其他草稿及可达证书lease保留。依据：`listener-workspace-draft.ts:17`、`listeners-view.tsx:119`、`:235`、`use-listener-persistence.ts:62`。
5. observation事件不丢弃在途bootstrap；workspace/snapshot/session/cert/settings事件才废弃旧写回并排队替换请求，generation迟到响应保护保持。依据：`bootstrap-context.tsx:47`、`:93`、测试`:224`、`:247`。
6. 严格域名/IP/CIDR/DNS匹配符合已确认合同，SOCKS5未命中时使用原目标；不以唯一端口猜测。依据：`routing.rs:70`、`:78`、`socks5/mod.rs:175`、`routing_tests.rs:103`、`socks5/tests/tcp.rs:17`。
7. 活动文档、测试和任务状态一致；Display 1MiB/8192、remote RPC 128KiB、fixedRoot/证书revision和默认5秒失联合同未改。

## 结论边界与后续

这不是整任务完成：#3恢复产品行为待确认，#9草稿DTO需独立设计，Android真机未连接NOT_RUN，外部链路/CI未执行。后续整任务关闭前需补齐剩余分支及适用验证，再取得整任务最终审查。

建议#9先形成可评审Draft DTO/API方案，不能直接让严格领域对象接受不完整编辑态；该建议已交由只读设计切片具体化。未发现需要立即修改当前实现的审查项。
