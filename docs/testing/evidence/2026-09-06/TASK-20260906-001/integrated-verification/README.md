# 集成验证与复测

- 任务：TASK-20260906-001；高优先级；本批执行日期 2026-09-06（Asia/Shanghai）。
- 最新验证：Rust 45 组 1647 项通过、0 失败/忽略；前端 73 文件 550 项通过。
- 类型、零警告 lint、fmt、严格 workspace Clippy、架构/依赖边界、500 行门禁、Next 构建和 diff 检查均通过。
- 测试期间 1354 个源码/配置文件无变化；另记录 20 个既有 OMX 历史文件。当前源码加合同文档共 1385 文件，快照逐字节一致。
- 整任务未完成：Socket 读取块绝对上限仍待用户明确数值；已实现子集独立代码复审 APPROVE、架构 CLEAR；新增 P0/P1/P2 均为0。

## 当前权威与资源

`resources/source-snapshot.zip` 是最新被测源文件、配置、内嵌资源和活动 fixture 的完整快照；每项来源、大小和 SHA256 见 `resources/source-inventory.json`。版本为 HEAD `5fd8bfbb5a048fba4f4ac9da483ec704b5264437` 加本次工作区修改；新 failure.rs 和会话回归文件也在快照中。`current-changes.patch` 辅助查看已跟踪文件差异，新文件以 ZIP 为准。

`outputs/validation/review-fixes/` 保存最新完整命令、起止时间、实际输出、测试计数和稳定性/快照比对。此前 `validation/` 根目录与 `followup/` 是前两阶段的历史检查；`resources/preceding-validation-snapshot/` 是当时实际快照，不冒充最新状态。原始失败日志保留，包括首次 Clippy 测试分号、源码尺寸中间态、有效 RED 与测试设置错误；以各阶段 README 解释为准。

`resources/prior-read-only-review/` 保存此前审查原因、非分配探针及当时实际测试补丁。初始 Socket 探针的空 host fixture 无效，后续 valid-fixture 日志才证明当前域校验接受极值；这些是历史定位材料。每项正式修复均另有本次根目录 RED/GREEN 与最终矩阵。

`outputs/reviews/` 保存独立审查及其复现。初次 code review 的 REQUEST CHANGES 保留，后续审查另存，不改写历史判断。

## 从快照准备环境

1. 将 `resources/source-snapshot.zip` 解压到新建独立目录，再将 `resources/documentation-gate-inputs.zip` 解压到同一目录；不要覆盖其他工作区。后者补齐架构检查实际读取的文档和访问的链接/目录目标，共120项，清单见 documentation-gate-inventory.json。仅检查存在性的目录保留当时真实目录项，不伪造内容文件。
2. 使用 `resources/environment.txt` 中的 Rust/Cargo/Deno 版本，准备仓库文档要求的 macOS SDK、编译工具和系统依赖。此处不复制系统工具本体。
3. 用快照的 `deno.lock`、`package.json` 执行 `deno install --frozen-lockfile` 准备前端依赖；用 `src-tauri/Cargo.lock` 执行 `cargo fetch --locked --manifest-path src-tauri/Cargo.toml` 准备 Cargo 缓存。实际测试使用已有缓存，Cargo 命令全部 `--offline`；新机器的获取步骤需要对应包源可用。
4. 执行 `python3 <此证据目录>/replay/run.py <源码目录> integrated-verification --verify-only` 验证快照，然后去掉 `--verify-only` 重跑本地矩阵。脚本写入新的临时日志目录，不修改历史证据。
5. 原工作区另外执行 `git diff --check`；解压的非 Git 快照不把该步骤算作 Git 差异验证。
6. 测试创建的本机监听器、任务及临时资源由测试清理；复测日志留在输出的临时目录，按需人工管理，不做全局环境清理。

完整命令和实际执行起止时间见 `metadata.json` 与每个执行 JSON；本用例的 replay 直接读取这些命令。脚本验证1354项源码/配置和31项合同文档的文件内容一致性，不下载依赖、不访问业务服务、不触发 CI。附加文档输入含当时任务元数据，只在独立快照中复原；正式任务入库后的元数据可以更新，不当成业务源码变化。已从两份 ZIP 解压到全新目录独立执行架构文档检查，PASS 输出见 outputs/documentation-archive-replay.log。

## 分层结论

- [Android 活动 Profile](../android-active-profile/README.md)：fake ADB/控制应答和字段断言通过，真机/TUN 未执行。
- [规则保存所有权](../rules-save-ownership/README.md)：真实组件、可访问名交互与异步状态回归通过，原生 App 点击/截图未执行。
- [错误与观测](../exchange-error-observation/README.md)：领域/边界回归及真实本机 TCP；完整输入字节、Server 实收、EOF 和终态另存于用例。
- [Socket 资源](../socket-resource-limits/README.md)：诊断预分配已修；读取块上限仍待确认，不能计作完整 PASS。
- Windows/Android 构建、长稳、真实外部链路、业务结果、CI/制品/发布：NOT_RUN，未用本地 PASS 替代。

父任务为 TASK-20260905-004 / final-verification / `docs/testing/evidence/2026-09-06/TASK-20260905-004/final-verification/`。本次保留已有统一 Pipeline/raw 特化和业务行为，新增六项审查边界及两个复审反例；父档案保持不可变。
