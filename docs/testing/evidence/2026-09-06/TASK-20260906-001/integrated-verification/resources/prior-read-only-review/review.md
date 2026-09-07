# 现有代码架构与逻辑审查

审查对象：/Users/codin/Code/gmofg-payment-proxy，HEAD 5fd8bfbb，2026-09-06。只读审查；仓库源码、配置和任务元数据均未由本轮修改。临时副本仅添加复现测试，生产源码保持相同。

结论：REQUEST CHANGES。两个 P1、四个 P2。独立架构 lane 的交付判断为 BLOCK（资源预算缺口），职责边界本身无依赖倒置或业务双真相。

## P1：Android LAN 热协调应用了未显式应用的配置

运行中的 LAN Profile 被编辑并仅保存后，状态查询读取最新持久化 Profile；LAN 地址变化触发 endpoint reconciliation，以该 Profile 重建完整请求并发送 apply_active/apply。目标应用或100%丢包配置可绕过应用修改按钮与危险确认。来源：application/facade/android/activation.rs:303、infrastructure/adapters/android_adb/endpoint_reconciliation.rs:84、reverse/preparation.rs:28。契约：docs/requirements.md:363-374。证据为已核对的静态完整调用链；未运行真机。

建议：热协调以不可变活动快照为准，只替换LAN端点；保存配置与活动快照不一致时要求显式应用。

## P1：Socket 参数允许不受预算约束的内存预分配

诊断事件容量与读取块大小只有非零校验。诊断队列按 VecDeque::with_capacity(capacity) 预分配，未受 diagnostic_memory_bytes 限制；Direct读取按 read_chunk_bytes 分配 Vec。有效host/port下，两个 u32::MAX 输入均通过领域校验。事件容量探针同时把诊断预算设为1024字节。未启动大内存Listener，OOM为源码可确认的分配风险，非已执行崩溃。

来源：domain/workspace/validation/listener/socket.rs:33、proxy/socket_relay/observer.rs:235、proxy/socket_relay/io.rs:25。建议按字节预算限制或移除预分配，使用可失败分配；绝对上限需依据确认的产品合同。

[有效配置探针输出](socket-resource-validation-valid-fixture.log)。初次探针默认host/port为空，失败属于fixture不合法，原始记录保留在 socket-resource-validation.log，不作为产品缺陷证据。

## P2：新建规则可重复提交

RuleSinglePairEditor.materialize 等待条件/动作命令时未进入提交锁，两次点击捕获同一旧父回调。即使首个保存已经置pending，第二个仍读取旧闭包中的false并发送另一条rule_id:null请求。后端对每条创建请求分配新identity。

来源：src/features/rules/rule-single-pair-editor.tsx:57、rules-view.tsx:79；application/facade/unified_rules.rs:49。临时UI复现期望一次创建，实际两次：[日志](rules-duplicate-reproduction.log)。建议锁覆盖materialize到save全过程，使用当前状态守卫。

## P2：迟到保存覆盖后续选择

保存A期间列表行仍可选择B；B已显示后A响应无条件写回selected/input。来源：rules-view.tsx:83、rule-definition-list.tsx:30。临时UI复现期望Second，实际HTTP combined：[日志](rules-race-reproduction.log)。建议异步写回绑定选择代次，并使交互状态与保存所有权一致。

## P2：Socket应答前EOF丢失结构化上下文

Socket reader真实EOF可返回None；核心fail函数知道Downstream/read，但返回Error::new，code/stage/direction全部None。Socket映射为无方向ProcessingFailed。连接仍失败，不属于错误成功。生产HTTP BufferedServerReader不返回None，本项不扩大为真实HTTP EOF误分类。

来源：exchange/exchange.rs:96、126，proxy/socket_relay/protocol_exchange.rs:217。额外核心断言期望Read，实际None：[日志](server-eof-reproduction.log)。建议保留已知阶段/方向，稳定错误码使用现有或另行确认的合同。

## P2：Socket外部包失败在ExchangeObservation丢失关联字段

observation::failed_with_context的HTTP分支记录external_*字段，Socket分支遗漏；UI解析器因而无法建立external_package_call。其他Socket诊断通道仍可能保留详情，不是全局错误丢失。

来源：exchange/observation.rs:323；src-tauri/src/runtime_logs/exchange_ui_layer.rs:382。按现有HTTP用例派生的Socket测试期望method/request_id等字段，实际None：[日志](socket-external-observation-reproduction.log)。建议统一两个协议的外部错误字段投影。

## 本次验证

- cargo test --offline --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features：45组，1640通过，0失败、0忽略。[日志](rust-tests.log)
- deno task test --reporter=dot：72文件、542通过。[日志](frontend-tests.log)
- strict workspace Clippy、cargo fmt、typecheck、ESLint零警告、架构门禁、500行门禁、git diff --check全部通过。
- 临时副本新增4条边界断言均失败，分别对应两个UI竞态和两处错误信息丢失；安全资源探针通过表示极值被接受，不表示资源风险通过验收。
- 检查1352份跟踪源码/配置前后大小与mtime，HEAD稳定、无变化；临时副本只修改了测试文件。[状态记录](source-stability.json)
- 未执行真实Android/TUN、真实外部业务、跨平台发布/制品、CI；未进行巨额内存分配。Windows/macOS实际交付不能由这些本地测试代替。

临时复现入口：frontend-reproduction 下使用 deno task test src/features/rules/rules-view.test.tsx -t 'review reproduction'；rust-reproduction 下从该副本根使用 cargo test --offline --manifest-path src-tauri/Cargo.toml -p intercept-proxy-exchange --lib 加对应测试过滤名。资源探针属于domain包。它们是本轮临时审查证据，不是已归档项目TASK验收。
