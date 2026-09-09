# HTTP Body 字符集修复合并与 Windows 构建

- 任务 ID：TASK-20260909-002
- 状态：进行中
- 优先级：高（用户要求远程合并和 Windows 交付）
- 任务日期：2026-09-09
- 创建时间：2026-09-09 12:40:48 +08:00
- 开始时间：2026-09-09 12:40:48 +08:00
- 最后更新时间：2026-09-09 12:40:48 +08:00
- 完成时间：未完成
- 创建路径：docs/tasks/pending/2026-09-09/merge-body-charset-develop-windows.md
- 归档路径：docs/tasks/completed/2026-09-09/merge-body-charset-develop-windows.md
- 关键词：HTTP、charset、GitLab、develop、Windows

## 背景、目标与范围

用户要求“提交 并创建merge请求到develop 合并信息为提交内容 然后merge到develop后 编译windows版本出来”。发布修复 f96f597573704967af1660aa7c2206a271de66e2；MR 合并至 develop 后从其实际合并提交构建当前 1.0.1 Windows MSI、NSIS、portable ZIP。
不包含 release/master 合并、版本升级、安装和业务验收。原主工作区的 QV 修改保持不变。

## 历史与当前事实

- 历史已验证：TASK-20260908-004 使用 android-windows-build 得到 Windows 包；本次重新检查配置和实际远程结果。
- 当前已验证：原修复任务 TASK-20260909-001，body-charset 用例，源码 f96f5975 已通过 69 测试及独立 APPROVE。
- 当前已验证：develop 从 1c88aeb8 更新到 2c784fbb，新增 capture-url；源码无冲突，任务与测试索引各一处冲突。
- 当前已验证：不同隔离工作区分配了相同 TASK-20260909-001；保留两份已完成档案及独立 case 路径，不改写历史资源，用任务路径和用例联合区分。

## 需求确认与就绪

明确授权 push、创建/合并 MR 到 develop、Windows 构建。合并提交文本采用待推送提交完整内容；增加 [skip ci] 控制自动 MR 流水线。仅手动触发已有 Windows 打包模式，其 Android companion 是打包资源前置作业，不运行独立 Android 发布或完整质量流水线。
示例：MR 源码包含 Body Shift_JIS 预览修复及最新 capture-url，MSI/NSIS/portable 标识 1.0.1。
问题、范围、输入输出、顺序和 PASS/FAIL 标准明确；无功能合同变化，进入实现时间 2026-09-09 12:40:48 +08:00。合并不是新缺陷修复，根因分析 N/A；原根因见原任务。

## 最小改动与最优设计

采用最小集成：合并最新 develop，仅保留双方索引；已有构建模式无需修改 CI 或全局设置。不引入新依赖、兼容层或构建路径。

## 小任务与验证计划

1. 集成 develop，锁内合并任务索引；抓包测试、类型检查和文档检查。
2. 提交并 push -o ci.skip，MR 标题/描述及 merge_commit_message 使用提交内容；固定 SHA 合并。
3. 核对 develop 祖先和树，API 手动构建 android-windows-build，轮询全部实际 jobs 到终态。
4. 下载确切版本安装包，验证哈希、PE 架构/版本和内嵌 APK；记录警告、未执行边界。
5. 独立整体对抗审查，通过后锁内归档。

原测试复用：TASK-20260909-001 / body-charset / docs/testing/evidence/2026-09-09/TASK-20260909-001/body-charset/。沿用源码预览合同，本次只改变远程合并与Windows构建证据；原资源不覆盖。

## 文档影响

任务及证据索引需要更新；用户指南、架构和产品文档无需额外修改，原功能文档保留。测试报文、真实链路、UI安装和业务交易 N/A（本任务为代码交付与构建）。

## 实施、验收与完成记录

进行中。证据用例 merge-windows-build；所有证据、测试、CI结果、修改文件与审查结论将在完成时填写。
