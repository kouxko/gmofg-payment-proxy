# 1.0.1 按 develop → release 合并并构建 Windows

- 任务 ID：TASK-20260908-004
- 状态：进行中
- 优先级：高（远端分支合并与Windows CI交付）
- 任务日期：2026-09-08
- 创建时间：2026-09-08 17:50:37 +08:00
- 开始时间：2026-09-08 17:50:37 +08:00
- 最后更新时间：2026-09-08 17:50:37 +08:00
- 完成时间：未完成
- 创建路径：docs/tasks/pending/2026-09-08/merge-1-0-1-develop-release-windows.md
- 归档路径：docs/tasks/completed/2026-09-08/merge-1-0-1-develop-release-windows.md
- 关键词：develop、release、1.0.1、GitLab、Windows、CI

## 需求确认与就绪

用户明确纠正顺序：“合并顺序不对 要先合并到develop 然后develop合并到release并触发ci创建windows程序”。原TASK-20260908-003只完成release-1.0.1→release的MR准备；本任务新增实际合并与Windows CI授权。原任务保持历史归档，不改写原结论。

输入：MR!9 source release-1.0.1，当前develop bb47b0eb、release10b3b576、发布分支6f1e3d48。输出：先发布分支合入develop，再develop合入release，最后在精确release头触发Windows构建并获得可下载程序。App101/1.0.1与Proxy1.0.1保持。范围不含额外产品改动、tag、GitLab Release对象、部署、真实Windows运行或业务验收。

当前CI配置Windows构建专用模式PIPELINE_MODE=android-windows-build，通过api/web触发。启用android_companion作为Windows内置APK构建依赖，再windows_build_only；coverage_gates、verify_windows与package_windows_unsigned排除，不运行完整测试/多平台或发布流程。依照当前配置执行，不凭历史缓存/并行实现补改CI。

需求就绪PASS，未确认实施项无；2026-09-08 17:50:37 +08:00进入执行。可复现示例：MR!9 retarget develop后merge；新MR develop→release merge；API pipeline ref=release且变量如上；两个job成功且Windows包含1.0.1 MSI/NSIS/portable。

## 现象、原因与修复边界

当前已验证：MR!9仍opened且目标release；两个目标分支尚未变化。现象为发布MR目标不符合用户最新明确顺序。根因是前任务将目标解释为release；本次以用户纠正为准，调整MR目标并按依赖顺序实际合并。不是程序缺陷，不改源码。未知：当前runner执行结果与Windows制品内容，必须远端终态与下载检查后判定。

## 最小与最优方案

直接修改现有MR!9目标为develop，保留提交及讨论；按GitLab merge模式保留历史，再创建develop→release MR并合并。每步使用预期head SHA与最新远端状态约束，merge commit使用[skip ci]避免自动完整流水线，最后显式调用构建专用pipeline。无force push、无重写历史、无新依赖；最小与最优一致。

## 小任务与验收

1. 主Agent登记与保存源分支/CI配置；只读Agent核查现有Windows job合同。
2. 主Agent把MR!9改到develop并合并；验证merged状态、merge SHA与祖先关系。
3. 主Agent创建develop→release MR并合并；验证source精确、release包含develop，版本仍1.0.1/code101。
4. 主Agent仅触发Windows构建专用pipeline，保存ID/URL/被构建SHA、jobs、trace、开始/结束状态与实际产物；下载验证格式/版本/ZIP内容及内置APK。
5. 独立整体对抗审查，主Agent同步文档/日期归档。构建期间可先做不改被测状态的检查。无产品编辑，不重复功能测试；必要失败诊断保留原输出，不伪装成功。

## 文档与测试

父任务：[TASK-20260908-003](../../completed/2026-09-08/prepare-release-1-0-1.md)，父用例release-version，父证据docs/testing/evidence/2026-09-08/TASK-20260908-003/release-version。复用版本检查与资源，改变合并顺序且新增Windows实际构建验收；原证据保持不变。

唯一正式用例：[merge-windows-build](../../../testing/evidence/2026-09-08/TASK-20260908-004/merge-windows-build/README.md)。测试资源为当前CI YAML/关联构建脚本快照与实际API输入/输出、远端trace/产物清单及必要下载资源。业务报文、网络Frame、设备截图N/A（无业务或设备交互）。

文档影响：任务/证据/索引需要更新；根README、需求、用户操作、架构、协议和Android说明无需更新，因为不改变产品合同。原任务不重新打开。小任务不逐项独立审查，集成远端证据后整体审查。

## 实施、验收与剩余风险

进行中。只有pipeline终态成功和Windows文件验证通过后才声明制品交付；Windows系统实际启动未在本任务环境执行，独立记录。
