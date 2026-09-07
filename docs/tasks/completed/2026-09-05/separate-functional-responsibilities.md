# TASK-20260905-003：功能职责与独立 crate 重构

- 任务 ID：`TASK-20260905-003`
- 状态：`已完成`
- 任务日期：`2026-09-05`
- 创建时间：`2026-09-05 21:38:55 +08:00`
- 开始时间：`2026-09-05 21:38:55 +08:00`
- 最后更新时间：`2026-09-05 22:33:24 +08:00`
- 完成时间：`2026-09-05 22:33:24 +08:00`
- 创建路径：`docs/tasks/pending/2026-09-05/separate-functional-responsibilities.md`
- 归档路径：`docs/tasks/completed/2026-09-05/separate-functional-responsibilities.md`
- 任务优先级：高；跨crate、配置原子事务、状态与生命周期重构。
- 关键词：职责、crate、配置事务、环境服务、Android协议、观测投影、Workspace、前端控制器

## 背景与需求确认

用户根据上一轮六项架构职责审查明确要求“处理一下 划分更合理 可以分为更独立的crate”。执行属于已授权的保持行为重构；不引入新产品默认、重试、兼容回退或改变配置/协议格式。用户已要求不要hash证据；本任务只记录必要计划、测试命令/结果与审查结论，不创建hash清单、源码快照、日志/APK归档或证据目录。

## 当前事实、根因和范围

1. 完整配置replace/reset挂在ExternalPackageApplicationPort上；包注册适配器承担Workspace、Settings和包的整体事务。根因为功能端口边界过宽。
2. selected Workspace由Shell/Android/Listener多处重复解释，首项fallback不同；统一选中事实及版本/刷新，保留Rust权威选择。
3. Android页面同时处理多查询轮询/事件/准入常量；分离后端准入能力和前端交互控制。
4. environment worker反向依赖facade私有写屏障；由独立EnvironmentConfigurationService拥有候选状态和worker，Facade仅委托。
5. RuntimePipelineAdapter混合规则事务、运行状态和Session/Capture展示构造；提取无基础设施依赖的traffic-observation投影crate，原执行/清理所有权不拆散。
6. Listener页面混合编辑会话与渲染；提取feature控制器，保留Rust Draft/前端证书lease/异步隔离。
7. Android wire模型和帧编码目前混在application展示合同中；提取android-control-protocol crate，ViewModel转换留application。版本2、operation、错误码/文案、1MiB控制帧及序列化字节不变。

实际已验证为职责/依赖问题；未将潜在状态分歧描述为已复现故障。最小复现是端口接收完整配置、worker引用facade、各页面不同selected逻辑以及Pipeline构造UI模型的当前调用路径。性能改善未测量、不作为验收。

## 需求就绪

PASS：目标/范围/输入输出及不变行为明确；未确认产品事项0。具体例子：配置恢复失败仍全量回滚，过期租约不会因重构自动恢复；不同页面同一selected快照一致；Listener输入中的草稿/冲突/lease不丢失。先完成架构共识，再进入源码实现。

## 最小改动与最优设计

- 仅移动文件：改动小但状态/业务所有权不变，不满足职责目标。
- 本次选择：两个真正独立crate + application具名服务/专用事务端口 + frontend控制器，共享合同保持。
- 全部拆成crate：Environment约7350行与AppError/EventHub/应用合同交织，需大范围新端口和错误模型；本次不选，避免application反向依赖。

## 小任务和所有权

| ID | 范围 | 负责人 | 依赖 | 验收 |
| --- | --- | --- | --- | --- |
| A | Environment服务、共享写屏障、完整配置专用端口/适配器 | backend executor | 规划批准 | 原子replace/reset与候选/取消/排空回归不变 |
| B | traffic-observation crate、Pipeline投影委托 | observation executor | 规划批准 | 原始报文/Session/Capture和错误行为回归不变 |
| C | Workspace共享上下文、Android和Listener前端控制器 | frontend executor | 后端能力字段稳定后集成 | 多页面选择、竞态、草稿、冲突/lease测试通过 |
| D | android-control-protocol crate、Android容量能力、共享接线/门禁 | 主Agent | 规划批准 | 独立crate测试、wire/application/adapter回归通过 |
| E | 集成、活动文档、整体独立审查 | 主Agent+独立reviewer | A-D终态收集 | 架构/type/lint/fmt/构建及受影响测试通过 |

## 测试与文档

实施前先锁定/运行现有受影响回归，缺覆盖处先写失败回归；每lane冻结后再集成。Rust使用根目录--manifest-path src-tauri/Cargo.toml。最终运行新crate测试、application/infrastructure相关全套、前端受影响测试、类型/lint/fmt/架构检查及桌面构建。用户已禁止额外证据材料，仅在本任务记录结果；不复制原历史资源。

需同步Cargo依赖门禁、模块图/当前架构和相关调用说明；测试路径迁移后同步对应结构检查，不删除检查或放宽合同。旧6项源码行数超限不作为本次完成前必须清理的独立目标，但不得新增。真机/外部服务行为未改；若发现影响实际协议行为则停止该方向重新核对，不把本地编译当真机验收。

## 实施与验收记录

- 2026-09-05 21:38:55 +08:00：登记；使用ralplan非交互架构评审，Architect→Critic顺序完成后切换原生executor并行实现。用户已经授权执行，不因技能默认的仅输出计划步骤再次要求确认；不启动未请求的持久Goal、不启动CLI/tmux。暂无源码修改。
- 未提交、未推送、未触发CI、未安装/发布。旧任务材料和无关修改保留。

- 2026-09-05 21:50:14 +08:00：实施前基线 PASS：`cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application --lib` 432项；infrastructure --lib 474项；`deno task test src/features/shell src/features/android-network src/features/listeners --reporter=dot` 31文件271项；Android聚焦11项通过。工作区未改源码。

- 2026-09-05 22:05:24 +08:00：架构共识完成；Architect修订后APPROVE，Critic顺序APPROVE。实施门禁PASS，原生executor A/B/C与root D按冻结合同并行实施。方案见`.omx/plans/separate-functional-responsibilities.md`。traffic-observation明确外层纯投影库；旧ApplicationConfigurationStorePort无查询职责，删除冗余生产writer；完整事务沿用唯一registry/lock/SQLite事务。
- 精确帧和公共错误回归在拆分前各1项PASS；拆分后Android相关39项PASS，新协议crate独立3项PASS，依赖方向门禁新增3项PASS。1 MiB边界和apply_active字节一致。

## 最终实施与验收

A–E 全部完成。Architect 修订后 APPROVE，Critic 顺序 APPROVE；三条执行线终态收集、集成并冻结后，独立 code-reviewer 整体审查 APPROVE，P0/P1/P2 均为0。

- `android-control-protocol` 独立拥有 v2 wire、机器状态和帧编码；Application 仅作 ViewModel/错误转换。操作白名单、apply_active、4字节大端前缀、精确1 MiB及公共错误码不变。
- `traffic-observation` 是仅供 Infrastructure 调用的纯投影库；Session/Capture/消息投影脱离适配器，解码、store/event、epoch及规则事务所有权不变。
- EnvironmentConfigurationService 拥有候选与worker；全部原配置写入/Android读租约/worker使用同一ConfigurationMutationCoordinator。
- 完整配置由 ApplicationBundleReplacementPort 单一路径写入；冗余生产配置端口删除。Adapter沿用同一registry/lock/SQLite原子事务，运行时仅在成功后更新，证书回滚保留。
- SelectedWorkspaceProvider 共享后端唯一selected及revision，拒绝无效选择与迟到查询，无首项fallback。
- Android controller统一刷新/事件/轮询与设备草稿，容量来自同一次后端owner快照；Rust常量与SQLite原子准入一致，Schema100原trigger未改。
- Listener View收敛到54行；控制器保留草稿/冲突/pending/串行intent及证书lease。
- 全仓Clippy发现既有Android engine测试一处Default类型写法警告，仅改为显式BTreeSet::default，无行为变化。

### 原任务和回归连续性

延续 [TASK-20260905-001](../../pending/2026-09-05/fix-reviewed-architecture-contracts.md) 的手动恢复、租约、严格路由和协议行为，以及 [TASK-20260905-002](listener-draft-dto-design.md) 的Rust Listener Draft/异步/证书lease行为。001原有Android真机验收待办保持不变，本次未将其标记完成。

沿用活动测试：application/android/tests.rs、requirements_tests/environment_configuration_*、requirements_tests/workspace_configuration*，infrastructure/adapters/android_adb/tests/manual_recovery.rs、pipeline/tests*、sqlite/tests/android_runtime_owner.rs，以及前端Listener transition-races/external-refresh/pending/certificate与Android device-isolation测试。新增协议精确输入断言与投影字段比较直接保存在测试代码中；不复制旧证据或改写历史预期。本次变化是职责/依赖，不改变原用例的业务预期。

### 最终验证结果

| 验证入口 | 实际结果与判定 |
| --- | --- |
| cargo test --quiet --manifest-path src-tauri/Cargo.toml -p intercept-proxy-application -p intercept-proxy-infrastructure -p intercept-proxy-host -p intercept-proxy-android-control-protocol -p intercept-proxy-traffic-observation | 1034项单元/集成全部PASS；含application435+39、host31、infra474+46、新协议3、投影6 |
| deno task test --reporter=dot | 71文件542项PASS。首次541/542，未改协议包焦点用例独立复查22/22及最终完整重跑均PASS；保留首次观察，不掩盖时序波动 |
| deno task typecheck / lint / build | 全部PASS；生产前端13页成功生成 |
| cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check | PASS |
| cargo clippy --quiet --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features -- -D warnings | PASS |
| deno task bindings / check:bindings | release构建和生成PASS；当前绑定fresh，连续生成一致；仅新增只读overview，原DTO不变 |
| deno task scan:architecture | PASS；新crate依赖门禁3项、生产边界80项fixture、runtime43项fixture、任务所有权及Socket/前端边界通过 |
| deno test -A scripts/deno-toolchain-contract.test.mjs scripts/check-architecture-docs.test.mjs | 11项PASS |
| deno task scan:source-size | 原6项超限保持，没有新增；不作为本次行为重构回归 |
| git diff --check | PASS |

测试在各lane冻结后执行；最后仅测试文件类型显式化，不改变被测生产实现。真实Android/原生WebView/外部服务验收N/A：本次没有安装、改变Kotlin/JNI或启动真实设备链路；不把本地测试当作001的真机完成证据。

现有源码尺寸限制：proxy/http/exchange_runtime/endpoints.rs566，domain/workspace/listener_model.rs546，android-network-view.test.tsx516，application/requirements_tests/listener_certificates.rs516，application/environment_configuration/listener.rs504，infra/adapters/listener_runtime/plan.rs504。未扩大此列表。

### 修改文件

- `docs/architecture/README.md`
- `docs/architecture/modules.md`
- `package.json`
- `scripts/check-architecture-boundaries.mjs`
- `scripts/runtime-crate-dependencies.mjs`
- `scripts/runtime-crate-dependencies.test.mjs`
- `src-tauri/Cargo.lock`
- `src-tauri/crates/android-control-protocol/Cargo.toml`
- `src-tauri/crates/android-control-protocol/src/error.rs`
- `src-tauri/crates/android-control-protocol/src/lib.rs`
- `src-tauri/crates/android-control-protocol/src/tests.rs`
- `src-tauri/crates/android-engine/src/socks5/tests/tcp.rs`
- `src-tauri/crates/application/Cargo.toml`
- `src-tauri/crates/application/src/android.rs`
- `src-tauri/crates/application/src/android/protocol_error.rs`
- `src-tauri/crates/application/src/android/runtime_owner.rs`
- `src-tauri/crates/application/src/android/tests.rs`
- `src-tauri/crates/application/src/coordination.rs`
- `src-tauri/crates/application/src/environment_configuration/lifecycle/worker.rs`
- `src-tauri/crates/application/src/environment_configuration/mod.rs`
- `src-tauri/crates/application/src/environment_configuration/service.rs`
- `src-tauri/crates/application/src/facade.rs`
- `src-tauri/crates/application/src/facade/android/runtime_owner.rs`
- `src-tauri/crates/application/src/facade/configuration.rs`
- `src-tauri/crates/application/src/facade/environment_candidates.rs`
- `src-tauri/crates/application/src/lib.rs`
- `src-tauri/crates/application/src/ports.rs`
- `src-tauri/crates/application/src/ports/application_bundle.rs`
- `src-tauri/crates/application/src/ports/external_packages.rs`
- `src-tauri/crates/application/src/ports/protocol_packages.rs`
- `src-tauri/crates/application/src/requirements_tests/environment_configuration_atomic_apply/application_wiring_contract.rs`
- `src-tauri/crates/application/src/requirements_tests/environment_configuration_validation/existing_active_listener_red.rs`
- `src-tauri/crates/application/src/requirements_tests/environment_configuration_validation/preview_contract_red.rs`
- `src-tauri/crates/application/src/requirements_tests/environment_configuration_validation/preview_security_red.rs`
- `src-tauri/crates/application/src/requirements_tests/environment_configuration_validation/runner_worker_red.rs`
- `src-tauri/crates/application/src/requirements_tests/environment_configuration_validation/selector_projection_red.rs`
- `src-tauri/crates/application/src/requirements_tests/external_package_lifecycle/support.rs`
- `src-tauri/crates/application/src/requirements_tests/protocol_package_lifecycle/support.rs`
- `src-tauri/crates/application/src/requirements_tests/support/application.rs`
- `src-tauri/crates/application/src/requirements_tests/support/fake_ports.rs`
- `src-tauri/crates/application/src/requirements_tests/support/protocol_package_portability.rs`
- `src-tauri/crates/application/src/requirements_tests/workspace_configuration.rs`
- `src-tauri/crates/domain/src/android_network.rs`
- `src-tauri/crates/infrastructure/Cargo.toml`
- `src-tauri/crates/infrastructure/src/adapters/android_adb/protocol.rs`
- `src-tauri/crates/infrastructure/src/adapters/bundle.rs`
- `src-tauri/crates/infrastructure/src/adapters/capture.rs`
- `src-tauri/crates/infrastructure/src/adapters/environment_configuration_validation_tests.rs`
- `src-tauri/crates/infrastructure/src/adapters/external_package_registry/application_bundle.rs`
- `src-tauri/crates/infrastructure/src/adapters/external_package_registry/application_port.rs`
- `src-tauri/crates/infrastructure/src/adapters/external_package_registry/mod.rs`
- `src-tauri/crates/infrastructure/src/adapters/mod.rs`
- `src-tauri/crates/infrastructure/src/adapters/pipeline.rs`
- `src-tauri/crates/infrastructure/src/adapters/pipeline/completion.rs`
- `src-tauri/crates/infrastructure/src/adapters/pipeline/mapping.rs`
- `src-tauri/crates/infrastructure/src/adapters/pipeline/message_projection.rs`
- `src-tauri/crates/infrastructure/src/adapters/pipeline/session.rs`
- `src-tauri/crates/infrastructure/src/adapters/protocol_package_import/tests.rs`
- `src-tauri/crates/infrastructure/src/adapters/workspaces.rs`
- `src-tauri/crates/infrastructure/src/adapters/workspaces/tests.rs`
- `src-tauri/crates/infrastructure/src/sqlite/android_runtime_owner.rs`
- `src-tauri/crates/traffic-observation/Cargo.toml`
- `src-tauri/crates/traffic-observation/src/lib.rs`
- `src-tauri/crates/traffic-observation/src/tests.rs`
- `src-tauri/src/commands/android.rs`
- `src-tauri/src/commands/mod.rs`
- `src/features/android-network/android-network-view-device-isolation.test.tsx`
- `src/features/android-network/android-network-view.test.tsx`
- `src/features/android-network/android-network-view.tsx`
- `src/features/android-network/android-runtime-owner-cards.test.tsx`
- `src/features/android-network/android-runtime-owner-view.test.tsx`
- `src/features/android-network/device-control-card.tsx`
- `src/features/android-network/profile-cards.tsx`
- `src/features/android-network/use-android-network-controller.test.tsx`
- `src/features/android-network/use-android-network-controller.ts`
- `src/features/android-network/use-android-owner-refresh.ts`
- `src/features/android-network/use-current-workspace-listeners.ts`
- `src/features/listeners/listeners-view.certificate-details.test.tsx`
- `src/features/listeners/listeners-view.certificates.test.tsx`
- `src/features/listeners/listeners-view.codecs.test.tsx`
- `src/features/listeners/listeners-view.external-refresh.test.tsx`
- `src/features/listeners/listeners-view.pending.test.tsx`
- `src/features/listeners/listeners-view.socket-contracts.test.tsx`
- `src/features/listeners/listeners-view.test.tsx`
- `src/features/listeners/listeners-view.transition-races.test.tsx`
- `src/features/listeners/listeners-view.tsx`
- `src/features/listeners/use-listener-controller.tsx`
- `src/features/rules/rules-view.test.tsx`
- `src/features/rules/use-rule-definition-source.ts`
- `src/features/shell/app-shell.tsx`
- `src/features/shell/navigation.test.tsx`
- `src/features/shell/selected-workspace-context.test.tsx`
- `src/features/shell/selected-workspace-context.tsx`
- `src/generated/rust-types.ts`
- `src/lib/ipc/client.ts`

任务管理文件：本任务文档、docs/README.md待办入口、docs/tasks/README.md完成入口。附加规划文件位于.omx/plans和.omx/context。当前架构文档及正常依赖门禁同步完成。

用户已明确不要hash证据：没有创建本任务hash清单、证据目录、日志/APK归档或源码快照，只保留任务与测试结果摘要。未提交、推送、触发CI、安装或发布。既有脏任务/证据索引与材料保留。

完成时间：2026-09-05 22:33:24 +08:00；验收结果：PASS_WITH_KNOWN_UNRELATED_SOURCE_SIZE_FAILURE。

## 本地提交记录

2026-09-05 23:03:33 +08:00：用户明确要求“提交”。已创建并核对本地提交 `refactor: clarify crate and feature responsibilities`，包含95个已审查的代码、测试、构建与当前架构文档文件；暂存内容与提交后文件范围一致，提交区为空。本轮未改变已验收的实现，沿用上一轮完整验证；提交前后范围和差异检查PASS。按既有偏好不记录提交hash，任务/证据档案及索引保留本地未提交。未推送、未触发CI。
