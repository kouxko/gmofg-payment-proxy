# TASK-20260904-006：GitHub 快速构建 Android 与 Windows 联合验证包

- 任务 ID：`TASK-20260904-006`
- 状态：`已完成`
- 任务日期：`2026-09-04`
- 创建时间：`2026-09-04 17:43:27 +08:00`
- 开始时间：`2026-09-04 17:43:27 +08:00`
- 最后更新时间：`2026-09-07 19:12:35 +08:00`
- 完成时间：`2026-09-07 19:12:35 +08:00`
- 创建路径：`docs/tasks/pending/2026-09-04/github-quick-android-windows-package.md`
- 归档路径：`docs/tasks/completed/2026-09-07/github-quick-android-windows-package.md`
- 关键词：`GitHub Actions`、`workflow_dispatch`、`Android Companion`、`Windows`、`quick build`、`artifact`、`cache`
- 任务优先级：`高`
- 优先级理由：任务涉及外部 CI、跨平台构建、桌面包内嵌 Android Companion 资源和可下载交付物；错误配置会产出缺少设备端组件或不可复用缓存的错误验证包。

## 背景、目标与需求确认

- 背景：现有 `windows-quick-build.yml` 只构建不含 Android resource 的 Windows exe；用户需要立即从 GitHub 获取同时完成 Android 与 Windows 编译的快速验证包。
- 目标：保留手动专用快速 workflow，仅执行 Android Companion 构建和 Windows executable 构建，将新 APK 嵌入 Windows 构建输入，并在最终 combined GitHub artifact 中同时提供 exe 与 APK；复用跨 workflow/job 的稳定缓存键。
- 范围：`.github/workflows/windows-quick-build.yml`、workflow 合同测试、操作文档、本次提交/push/手动 dispatch 与远程 run/artifact 验证。
- 不在范围：Coverage、macOS、完整 Verify、MSI/NSIS、签名、tag、GitHub Release、其他 workflow、GitLab CI、部署或发布。
- 需求确认记录：`2026-09-04 17:36:00 +08:00` 用户明确要求提交并推送 GitHub，只触发 Android + Windows 编译，使用全局缓存并尽快获得验证包。
- 未确认事项：零；“包”按现有 quick workflow 语义实现为 GitHub artifact，包含未签名 Windows exe 与同次构建的 Android APK。

## 需求就绪检查

- 问题、目标和成功结果：`PASS`
- 范围与不在范围：`PASS`
- 输入、输出和状态变化：`PASS`；输入为指定 Git ref，最终交付 artifact 为 `Intercept-Proxy-quick-validation-x64`，包含 `intercept-proxy.exe` 和其可直接解析的 `resources/android-companion.apk`；Android job 另上传一个 APK 中间 artifact 供 Windows job 下载。
- 错误行为：`PASS`；Android 构建、资源 staging、Windows 构建、OpenSSL 依赖检查或 artifact 缺失任一失败时 workflow 失败。
- 具体示例：`PASS`；手动 dispatch 后只出现 `android-companion` 与 `build-windows-executable` 两个 job，最终 combined artifact 同时包含 `intercept-proxy.exe` 和 `resources/android-companion.apk`。
- 可重复 PASS/FAIL 验收：`PASS`；workflow 合同测试、YAML 结构检查、远程 job 清单、run 结论和 artifact 文件清单直接判断。
- 改变实现方向的未确认事项：`0`
- 进入实现时间：`2026-09-04 17:43:27 +08:00`

## 最小改动与最优设计

| 方案 | 分析 |
| --- | --- |
| 同时触发 `ci.yml` 与原 quick workflow | 会额外运行 Coverage/Verify，Android 产物也不会进入 Windows 构建，不满足只跑两类编译。 |
| 触发完整 `windows-release.yml platform=windows` | 会执行完整 Windows 验证、安装包构建等额外流程，不能快速出包。 |
| 扩展独立 quick workflow | 增加 Android job，Windows job下载并 stage APK后构建，单 artifact 输出 exe + APK；继续无 push/tag 自动触发，范围最小，采用。 |

缓存设计：Android 使用可信手动分支可写的 Gradle 缓存；Deno 使用 setup action 缓存；Next.js 与完整桌面流程复用同一 hash key；Rust 使用与桌面流程相同的 `desktop-${{ runner.os }}` shared key、禁用 job-id key，使 quick/release/verify 可复用同一 Windows Cargo cache。缓存只加速，不作为成功证据；GitHub 的默认分支/当前分支可见性边界仍保持。

## 小任务、测试与审查

| ID | 内容 | 状态 | 验收 |
| --- | --- | --- | --- |
| T01 | 修改 quick workflow 的 Android/Windows 依赖与 combined artifact 输出 | 已完成 | 仅两个 job，APK 进入 Windows 构建输入 |
| T02 | 更新 workflow 合同测试和操作文档 | 已完成 | 旧“Windows-only/空资源”合同删除，新合同 7/7 PASS，独立复审 APPROVED |
| T03 | 本地验证、提交并安全推送 GitHub 新分支 | 已完成 | 后续同合同分支 codex/windows-executable-20260906-fa4e154f 已实际完成提交/推送；本轮只复核，不新推送 |
| T04 | 手动 dispatch、监控 run、核验 artifact | 已完成 | run34020524291 workflow_dispatch成功；exe/APK重新下载并核验；本轮恢复最终仅手动入口，合同3/3及独立审查通过 |

测试计划：运行 workflow 合同测试、Deno lint/格式或适用 YAML 检查、`git diff --check`；远程使用 `gh workflow run` 指定本次 ref，仅触发 quick workflow，查询 job 与 artifact 清单并下载核验文件类型。

对抗审查计划：确认没有 `push`/`pull_request` 自动触发，没有 Coverage/macOS/installer/完整 Verify job，Windows 构建不再用空 resource override，缓存键不含 job ID，任一步失败不会上传伪成功 artifact。

## 实施记录、修改文件与验收结果

- `2026-09-04 17:43:27 +08:00`：核对 GitHub 分支、workflow 触发边界和现有 quick/full release 合同，登记任务。
- `2026-09-04 18:16:27 +08:00`：快速 workflow 收敛为 Android 与 Windows 两个 job；Android 产出 APK 供 Windows 下载/stage，Windows 输出 combined artifact；移除空 resource override。合同测试 7/7、Deno fmt、ESLint、Rust fmt/严格 Clippy 和 diff check 均 PASS；待独立审查、commit、push 与远程 run。
- `2026-09-04 18:20:31 +08:00`：独立审查发现最终 artifact 的 APK 不在 exe 运行时可解析路径；已改为 `resources/android-companion.apk`，并补充与完整 workflow 共享的 Next.js 缓存键，待重跑验证和复审。
- `2026-09-04 18:23:11 +08:00`：重跑合同 7/7、Deno fmt、ESLint、YAML 解析和 diff check 全部通过；独立复审 APPROVED，P0/P1/P2=0，确认触发、依赖、缓存和 artifact 无阻塞。
- `2026-09-04 18:25:09 +08:00`：首次 push 新分支不会自动触发任何 workflow；`gh workflow run` 因该 workflow 未在 GitHub 默认分支注册而返回 404，确认无 run 产生。为不修改 main 或触发完整 CI，临时增加仅匹配 `codex/http-chunked-quick-validation-20260904` 的单次 push 触发；构建启动后删除，最终代码恢复为仅手动入口。

修改文件：`.github/workflows/windows-quick-build.yml`、`scripts/windows-build-only-workflow.test.mjs`、`docs/onboarding-guide.md`、本任务文档。

附加文件：[本轮完整证据](../../../testing/evidence/2026-09-07/TASK-20260904-006/manual-trigger-and-artifact/README.md)。

- 验收结果：`PASS`；本轮手动入口修复及后续成功构建产物复核通过。
- CI、push、发布：用户已授权本次 GitHub push 与指定 quick workflow 手动运行；其他 workflow、tag、Release、部署不授权。

完成总结：最终仅手动触发的联合构建入口已恢复；后续同合同成功run的原始产物重新核验并归档，本轮未推送或触发新CI。


## 新分支续办（2026-09-07 18:43:33 +08:00）

- 用户要求：切换到新分支处理当前6项待办。已从 develop 创建 codex/complete-pending-tasks-20260907，起点 caa36022；保留原有 gmofg_cps 与 http-finalize 未跟踪目录，不纳入本次工作。
- 执行范围：原任务已确认的本地修复、证据补齐、验证和状态收尾；本轮不从历史记录推导新的 push/CI/tag/Release 授权。
- 需求就绪复核与步骤：当前已验证：GitHub quick run 34020524291 两个 job 成功；最终承诺的仅手动入口尚未兑现，workflow 和测试仍锁定一次性 push 分支。根因是临时启动路径完成构建后未移除。最小与最优方案一致：先把现有合同测试改为严格仅 workflow_dispatch 并记录 RED，再删除临时触发器；复用原成功 run，核验并归档 artifact，不新增 job/缓存/依赖，不触发新 CI。验收：合同 GREEN、静态检查、历史 run 原始终态/产物实物、新旧 workflow 除删除临时触发块外一致、独立整体复审。
- 优先级仍为高，适用受影响验证与独立整体审查；任务级完成门禁未满足则保留未完成状态。
- 测试证据：新增执行使用 docs/testing/evidence/2026-09-07/TASK-20260904-006/ 下独立用例，原证据不覆盖。


## 续办完成与归档（2026-09-07 19:12:35 +08:00）

- 本轮删除4行临时push触发器并恢复仅workflow_dispatch；RED2通过/1失败，GREEN3/3，YAML解析/fmt/零警告ESLint/归档重放/独立审查APPROVE。成功run34020524291为workflow_dispatch，Android与Windows两job均成功，重新下载exe/APK实物完整且布局正确；当前jobs与该成功run逐字一致。本轮没有新push/CI/Release，也未进行Windows运行启动；后者不属于原quick artifact合同。
- 证据与复测：[README](../../../testing/evidence/2026-09-07/TASK-20260904-006/manual-trigger-and-artifact/README.md)、[metadata](../../../testing/evidence/2026-09-07/TASK-20260904-006/manual-trigger-and-artifact/metadata.json)、[独立审查](../../../testing/evidence/2026-09-07/TASK-20260904-006/manual-trigger-and-artifact/independent-review.md)。
- 原任务/用例资源保持不变；本次用例与复测脚本随档案保存。任务范围全部验收满足，归档至实际完成日期2026-09-07。
