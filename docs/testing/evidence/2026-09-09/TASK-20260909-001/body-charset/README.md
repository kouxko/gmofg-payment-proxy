# HTTP Body 字符集预览回归

- 任务：TASK-20260909-001；用例：body-charset。
- 结果：受影响验证 PASS（69 tests）；[整体独立审查 APPROVE](outputs/review.md)，P0/P1/P2 = 0/0/0。
- 环境/版本：[environment.json](outputs/environment.json)。基线 `1c88aeb8`，分支 `hotfix/fix-http-body-charset-preview`。
- 目标：Shift_JIS 日文在 Exchange Body 正常显示；业务原字节、非 UTF-8 Mock 门禁及观测预算不变。

## 输入与资源

- [Shift_JIS 原始字节](inputs/receipt-shift-jis.bin)、[hex](inputs/receipt-shift-jis.hex)、[HTTP Header 原始 CRLF](inputs/response-header.txt)、[期望](inputs/expected.json)。
- 向量为按用户截图场景构造的 `売上票（署名）保管正常完了。`，不是截图原报文；活动 fixture 版本 1，路径见 [资源说明](resources/README.md)。
- [APK 构建前置资源](../../../2026-09-07/TASK-20260904-006/manual-trigger-and-artifact/resources/artifact/resources/android-companion.apk) 复用 TASK-20260904-006/manual-trigger-and-artifact 的不可变归档，SHA-256 完全一致。只满足 Tauri build-script，不运行 Android。
- [最终被测源码清单](resources/source-files.json)、[SHA-256](resources/source-sha256.json)、[源码快照](resources/source/)；基线加这些按仓库相对路径保存的源码可恢复本次实现。

## 执行与结果

1. 原 producer 的 non-UTF8 hex/预算两例 RED：[日志](outputs/producer-red.log)。
2. 原 consumer 的日文/非法编码两例 RED：[日志](outputs/consumer-red.log)。
3. 修复后 Exchange 33、codec 6、Mock 1 PASS：[第一组检查](outputs/initial-verification/results.json)。其中 Clippy 发现测试辅助代码使用 format/collect，单独修正后复验；原测试文件按 SHA 校验后保留在 resources/consumer-test-before-clippy.rs。
4. 最终 consumer 21、前端 8、严格 Clippy、fmt、typecheck、ESLint、架构边界/文档、diff 全部 PASS：[最终检查](outputs/final-verification/results.json)。两组每项检查均记录实际命令、时间、退出状态与源码稳定性。
5. [实际 DTO 记录](outputs/actual-records.json) 与 [逐字段比较](outputs/comparison.json)：四个带 Context 的阶段日文等于期望，body_is_utf8 仍 false，输入 byte snapshot 等于期望 hex。
6. [绑定生成日志](outputs/bindings.log)：只更新注释，公共字段未变化。

失败和限制完整保留：独立 worktree 起初缺 build-script APK；新增测试一处 AppError 字段访问错误已修正。macOS debug lib-test 链接器提示 `__eh_frame section too large`，测试仍通过。以上均未改写为功能测试成功。

## 从零复测

1. 使用本分支源码，按项目根 README 准备 Rust、Deno/Node 和 node_modules。历史复现可在 resources/base-commit.txt 对应基线的新工作区按相对路径还原 resources/source/，并保留本证据目录的复测脚本。
2. 若 `src-tauri/resources/android-companion.apk` 不存在，将上方已有归档 APK 复制到该路径；保留既有文件，不覆盖其他资源。
3. 从仓库根执行：

```bash
python3 docs/testing/evidence/2026-09-09/TASK-20260909-001/body-charset/replay/run.py
```

脚本默认创建新的 `/tmp/body-charset-时间/` 输出，拒绝覆盖已有目录。可用第一个参数指定新的输出目录，后续参数只选检查（如 `consumer clippy`）；无检查参数则全部执行。本次只复用原工作区 Cargo target 与 node_modules 缓存，不是必需的固定路径。

成功条件：所有所选检查退出 0 且 source_stable=true；功能断言全部通过。失败时保留日志和 results.json；不要覆盖此归档的期望或输出。无外部服务/数据库/设备状态需要恢复；可删除复测新生成的临时输出和本次新增的忽略构建资源。

## N/A 与适用范围

- 网络 Server/App 实际发送接收、TCP chunks、真实设备与业务结果：N/A，本次改只读观测，未改传输与协议包；不能把本次本地 PASS 当用户真实链路验收。
- UI 手动截图/Windows启动：N/A，前端生产组件未改，使用现有组件的 DOM 可见性回归。
- Listener 强制 codec、全部 charset 重设计、UTF-8 本来可显示的投影：不在本次 hotfix 范围。
- 安装/发布/远程 CI：NOT_RUN。重建 App 并产生新请求后生效，不追溯修复已保留的旧内存记录。

归档任务：[TASK-20260909-001](../../../../../tasks/completed/2026-09-09/fix-http-body-charset-preview.md)。
