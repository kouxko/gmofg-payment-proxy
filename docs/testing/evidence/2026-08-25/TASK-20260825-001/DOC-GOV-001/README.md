# DOC-GOV-001：项目 AGENTS 治理规范验证

## 测试信息

- 任务 ID：`TASK-20260825-001`
- 执行时间：2026-08-25 16:41:05 +08:00
- 测试前基线提交：`a5c6b1b739333816667584648ee6734179d8c853`
- 结果：PASS

## 测试目的

验证根目录 `AGENTS.md` 已覆盖当前确认的任务治理规则，不包含隐藏 Push、额外 CI、默认需求实现、
工作区或文件 Hash 要求，并且 Markdown diff 没有空白错误。

## 被测文件

- `AGENTS.md`
- `docs/README.md`
- `docs/tasks/completed/2026-08-25/generate-project-agents-governance.md`
- `docs/tasks/README.md`
- `docs/testing/evidence/2026-08-25/TASK-20260825-001/DOC-GOV-001/README.md`
- `docs/testing/evidence/2026-08-25/TASK-20260825-001/DOC-GOV-001/metadata.json`

## 执行步骤和命令

```bash
git diff --check -- AGENTS.md docs/README.md

test -f docs/tasks/completed/2026-08-25/generate-project-agents-governance.md
test -f docs/tasks/README.md
test -f docs/testing/evidence/2026-08-25/TASK-20260825-001/DOC-GOV-001/README.md
test -f docs/testing/evidence/2026-08-25/TASK-20260825-001/DOC-GOV-001/metadata.json

if rg -n '[[:blank:]]+$' \
  docs/tasks/completed/2026-08-25/generate-project-agents-governance.md \
  docs/tasks/README.md \
  docs/testing/evidence/2026-08-25/TASK-20260825-001/DOC-GOV-001/README.md \
  docs/testing/evidence/2026-08-25/TASK-20260825-001/DOC-GOV-001/metadata.json; then
  exit 1
fi

jq empty docs/testing/evidence/2026-08-25/TASK-20260825-001/DOC-GOV-001/metadata.json

test -f docs/tasks/completed/2026-08-25/generate-project-agents-governance.md
test -f docs/tasks/upstream-multi-ca-pem-bundle.md

rg -n '^## ' AGENTS.md

rg -n \
  '默认禁止触发任何远程 CI|只触发明确的 Windows Build Workflow|默认不 Push|需求零假设|强制对抗审查|docs/tasks/completed|docs/testing/evidence' \
  AGENTS.md

rg -n '哈希|Hash|hash|SHA-256|SHA256|sha256' AGENTS.md
```

## 实际结果

- 跟踪文件 `git diff --check`：退出码 0，无输出。
- 未跟踪归档和证据文件直接读取检查：全部存在，无尾随空白。
- `metadata.json`：`jq empty` 通过。
- 完成索引和现有待实现任务链接：目标文件均存在。
- `AGENTS.md`：共 18 个一级治理章节。
- 已找到需求零假设、测试证据、强制对抗审查、日期归档、默认不 Push、默认禁止 CI 和仅 Windows
  Build 条款。
- 未找到工作区或文件 Hash 要求。
- Markdown LSP：N/A；当前环境没有适用的 Markdown LSP。
- 网络报文、Frame、Decode、Rules、Encode、截图：N/A；本任务是治理文档任务，不执行网络功能。

## 对抗审查

- 首轮：`REQUEST CHANGES`，P0/P1/P2 为 0/5/2。
- 二轮：`REQUEST CHANGES`，剩余 P1 为 2。
- 用户补充：测试状态记录不得使用 Hash。
- 最终复审：`APPROVE`，P0/P1/P2 为 0/0/0。
- 最终档案审查首轮：`REQUEST CHANGES`，发现证据时间和未跟踪文件检查不足。
- 最终档案验证：已以 `a5c6b1b` 为基线重新执行。

## 复测方式

在仓库根目录重新执行“执行步骤和命令”中的命令，并确认输出符合“实际结果”。
