# 1.0.1 发布分支与 GitLab MR 准备

- 任务 ID：TASK-20260908-003
- 状态：进行中
- 优先级：高（发布分支整合与跨语言版本合同）
- 任务日期：2026-09-08
- 创建时间：2026-09-08 17:25:37 +08:00
- 开始时间：2026-09-08 17:25:37 +08:00
- 最后更新时间：2026-09-08 17:25:37 +08:00
- 完成时间：未完成
- 创建路径：docs/tasks/pending/2026-09-08/prepare-release-1-0-1.md
- 归档路径：docs/tasks/completed/2026-09-08/prepare-release-1-0-1.md
- 关键词：release、develop、1.0.1、versionCode101、GitLab、版本

## 背景、范围与需求确认

用户明确要求使用最新develop创建1.0.1发布分支，把现有release合入新分支避免冲突，将App与Proxy均设为1.0.1并提交GitLab，创建标题“1.0.1发布”的MR。此前明确Android versionCode应为101。

当前已验证：hotfix MR !8 已于2026-09-08 17:20:56 +08:00合入develop；最新develop=bb47b0ebb7508e8b4684a4192cbf8eb8d78b3b59，release=10b3b576123c6c3f4e44868f7fc4e86e53a67208。从前者创建隔离worktree分支release-1.0.1，发布MR目标release。现有本地release分支占用同名引用，使用release-1.0.1避免release/1.0.1引用前缀冲突。

范围：整合release历史、主Android APK版本1.0.1/code101、Proxy package.json/Tauri/Cargo工作区版本及相应lockfile一致性、必要配置验证、独立审查、提交/推送/MR创建/日期归档。
不在范围：真正合并MR、Git标签、Release对象、部署、CI、全平台制品构建或业务验收；不改协议包/示例/数据库Schema版本，不重启用户活动VPN。

需求就绪：输入两条精确分支，输出包含两者祖先且MR无冲突的新分支；所有主应用版本1.0.1，Androidcode101；错误不增加回退。未确认实现事项无；2026-09-08 17:25:37 +08:00进入实现。示例：Cargo metadata主包和workspace包版本1.0.1，AndroidGradle versionName1.0.1/versionCode101；MR title精确1.0.1发布、target=release、状态opened。

## 历史与根因适用性

复用 [TASK-20260908-002](fix-android-vpn-local-tcp.md) 的版本101/1.0.1约定及已合入LAN修复。原证据：docs/testing/evidence/2026-09-08/TASK-20260908-002/version-1-0-1，原APK版本和签名门禁已通过；本任务不把该结果改写为新的最终制品/业务验收。当前任务仅发布配置整合，不是新缺陷修复，根因分析N/A。

## 最小方案与最优设计

通过正常merge保留release/develop两边历史；如有冲突逐文件处理并保留有效记录。预合并树与develop产品内容相同，只有release独有祖先需整合。版本在各工具既有权威配置同步，Cargo lock仅更新内部包版本；不创建新版本框架、不添加依赖。最小与最优一致。

## 小任务、测试与审查

1. 主Agent：登记、release合入、来源祖先/树比较；版本定位Agent只读确认权威路径，不编辑共享文件。
2. 主Agent：同步应用版本和Cargo锁文件；检查JSON/TOML/Gradle、实际Cargo metadata消费、locked离线解析、所有内部包版本及外部依赖不变；按纯版本变更风险不重复完整功能/设备/全平台构建。
3. 独立Agent：全部修改和验证集成后整体对抗审查，APPROVE后交付。
4. 主Agent：提交带[skip ci]、push -o ci.skip、创建标题1.0.1发布的MR到release，验证头SHA、可合并、流水线skipped和零jobs；完成任务/证据索引与日期归档。

## 文档影响

任务/证据/完成索引需更新。其他产品文档不硬编码当前发布版，协议/架构行为不变，无需同步历史版本或业务手册。版本定位若发现其他当前权威位置再补充。

## 实施与验收

进行中。CI未授权，不执行。整个任务的完成表示发布分支与MR准备完成，不表示1.0.1已部署或真实业务验收通过。

版本定位发现scripts/build-macos-universal.mjs的DMG产物名仍硬编码1.0.0，属于同一Proxy发布版本合同，纳入本次同步为1.0.1；保持既有命名结构，不额外重构脚本。应用13个Cargo工作区包全部继承统一版本，锁文件相应更新13条，仅版本值变化，无registry依赖改动。

本地验证完成：版本一致性、13项内部lock变化及外部依赖不变、locked/offline/no-deps实际解析、5合同测试、脚本lint/格式/差异检查PASS。全依赖offline元数据缺少memfd缓存的初次失败已单列，不冒充完整构建。证据：[release-version](../../../testing/evidence/2026-09-08/TASK-20260908-003/release-version/README.md)。等待独立整体审查及远端MR操作。

独立code-reviewer整体审查APPROVE，发现项0；再次核对配置和资源一致性，通过deno check。见 [独立审查](../../../testing/evidence/2026-09-08/TASK-20260908-003/release-version/outputs/review.md)。下一步为已授权的提交、推送与MR创建，完成后核验远端状态并归档。
