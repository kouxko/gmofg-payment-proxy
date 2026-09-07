# TASK-20260907-003：构建 HTTP Finalize Windows 候选包

- 任务 ID：TASK-20260907-003
- 状态：进行中
- 任务日期：2026-09-07
- 创建时间：2026-09-07 17:11:19 +08:00
- 开始时间：2026-09-07 17:11:19 +08:00（调查/执行准备）
- 最后更新时间：2026-09-07 17:47:12 +08:00
- 完成时间：未完成
- 创建路径：`docs/tasks/pending/2026-09-07/build-http-finalize-windows.md`
- 归档路径：`docs/tasks/completed/<完成日期>/build-http-finalize-windows.md`
- 任务优先级：高
- 优先级理由：涉及远端构建交付或协议/签名及跨模块验收，执行完整适用流程。
- 关键词：HTTP、Wasm、Finalize、CPS、Windows、GitLab

## 背景、目标和范围

现有 GitLab API 手动 Windows 构建及依赖 Android Companion；确认精确提交、最终 jobs、下载并检查 Windows 产物。

## 需求确认记录

用户明确要求“创建windows构建流程”；GitLab feature/0907-001-http-finalize 的 183bccfb43c6a4903c315767e4b761fc57ceb08d，PIPELINE_MODE=android-windows-build。

仅授权的现有 Windows 构建可触发 CI。CPS 是本地实现，不因前一项交付授权而自动提交、推送、创建 MR、安装到运行 App 或触发新的 CI。不修改 CPS Android 源码或真实支付系统。

## 需求就绪

就绪：已核对 .gitlab-ci.yml 的专用 API 模式和两个 jobs、输入 SHA/branch、用户 CI 授权与成功标准；无产品代码修改。

## 当前事实、历史和原因

当前已验证：TASK-20260907-001 已实现通用 Finalize；GitLab MR !3 的单独提交为 183bccfb，目标 develop，无冲突。历史 Windows 产物仅供流程参考，不作为本任务结果。CPS 历史方案须由当前 /Users/codin/Code/jp_gmofg_cps 源码重验。新增能力/构建交付不直接等同缺陷修复；适用现象、复现、候选原因、根因在发现实际故障时追加，暂 N/A。

## 最小改动与最优设计

优先复用现有构建模式与 HTTP Component/WIT/JSON/Finalize 边界，不增加第二套 Proxy Pipeline、兼容回退或未约定默认成功。Windows 只调用现有流程；CPS 使用独立业务包承担字段/签名，Host 保持通用。源合同核验后细化文件所有权、错误模型和向量。

## 小任务与计划

1. 核对合同与输入、保存来源；主 Agent 协调，独立源码调查可并行。
2. 就绪后执行构建或包实现，分配不重叠文件；不在任务锁中运行构建。
3. 定向验证、实际产物和资源快照；全部实现结束后整体独立审查。
4. 同步文档、保存证据并按完成日期归档。小任务不额外开启审查，整批集成后执行一次整体审查。

## 验收标准

PASS：仅 android_companion/windows_build_only 执行且成功；Windows MSI/NSIS/portable 及伴随 APK 资源完整，保存 pipeline/job/sha/原始输出/下载摘要。Windows 实机运行不在范围。

## 文档与测试计划

任务、docs入口/完成索引/证据索引按锁更新。CPS 包需 README、实际 Manifest/构建命令/字段算法与限制；Windows 保持既有操作文档，差异发现后再修正。正式用例存于 `docs/testing/evidence/2026-09-07/TASK-20260907-003/`，含环境、输入、资源、预期实际、日志、复测和清理。不适用的真实设备、生产 TLS、真实支付、UI和跨平台运行标记 N/A 或 NOT_RUN，不生成占位证据。

## 实施与完成记录

- 2026-09-07 17:11:19 +08:00：登记；尚无构建或包验收结论。
- 修改文件：本任务文档与 docs/README.md 登记入口。
- 附加文件、测试结果、CI情况、审查结果、完成总结：执行后追加，当前未完成。

## Windows 构建执行

- 2026-09-07 17:19:38 +08:00：已通过 API 创建 [34143](http://172.16.3.60/other/intercept_proxy/-/pipelines/34143)，精确 SHA `183bccfb43c6a4903c315767e4b761fc57ceb08d`，ref `feature/0907-001-http-finalize`，变量 PIPELINE_MODE=android-windows-build。
- 已核对 jobs 仅 android_companion（104425）与 windows_build_only（104426）；未启动 verify/coverage/其他平台任务。
- http_contract 负责只读监控与下载检查，主 Agent 负责最终证据和归档。未改CI配置；包实现后不自动复用此次授权再次触发CI。

## 用户调整范围

- 2026-09-07 17:47:12 +08:00：用户明确“不用监听构建 专注于功能正确性”。停止本地 pipeline/job 轮询与产物下载跟踪，远端已创建的 CI 不作取消操作。最后已观察 Android job 成功、Windows job 运行中且前端构建/TypeScript检查通过；这些不是 Windows 最终构建或产物验收结论。后续交付验收不再作为本轮完成条件。保留已有原始观察，不继续查询最终状态。
