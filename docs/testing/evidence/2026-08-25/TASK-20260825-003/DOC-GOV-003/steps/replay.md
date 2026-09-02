# 复测步骤

1. 从仓库根目录打开 `AGENTS.md`。
2. 确认存在“测试资源必须成为任务归档的一部分”和“后续任务必须引用和复用原测试”章节。
3. 确认规则区分活动 fixture 与不可变证据快照。
4. 确认不可复制外部依赖使用可重建信息记录。
5. 确认 `derived_from` 使用父任务、父用例和父证据路径。
6. 确认兼容修改与替代旧合同采用不同复测流程。
7. 确认 `docs/testing/evidence/README.md` 包含本用例及父用例关系。

```bash
rg -n \
  '测试资源必须成为任务归档的一部分|后续任务必须引用和复用原测试|活动 fixture|不可复制依赖|derived_from|明确替代旧合同' \
  AGENTS.md

rg -n 'TASK-20260825-003|DOC-GOV-003|TASK-20260825-001|DOC-GOV-001' \
  docs/testing/evidence/README.md

jq empty \
  docs/testing/evidence/2026-08-25/TASK-20260825-003/DOC-GOV-003/metadata.json
```
