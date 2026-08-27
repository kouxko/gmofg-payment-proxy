# DOC-GOV-002：小任务对抗审查门禁验证

## 测试信息

- 任务 ID：`TASK-20260825-002`
- 执行时间：2026-08-25 16:52:35 +08:00
- 结果：PASS

## 被测范围

- `AGENTS.md`
- `docs/tasks/completed/2026-08-25/optional-subtask-adversarial-review.md`
- `docs/tasks/README.md`
- `docs/README.md`

## 复测命令

```bash
rg -n '小任务的针对性对抗审查是可选项|不构成固定提交门禁|全部小任务完成后必须进行一次整体对抗审查' AGENTS.md

if rg -n '每个小任务完成后进行针对性对抗审查|完成对应对抗审查的小任务' AGENTS.md; then
  exit 1
fi

test -f docs/tasks/completed/2026-08-25/optional-subtask-adversarial-review.md
test -f docs/tasks/README.md
```

## 实际结果

- 小任务针对性对抗审查：明确为可选，不构成固定提交门禁。
- 高风险小任务：建议优先审查，但仍为可选项。
- 整体任务最终对抗审查：继续强制，必须取得 `APPROVE`。
- 小任务完成：不再要求每个小任务都先完成对抗审查。
- 整体独立对抗审查：APPROVE，P0/P1/P2 为 0/0/0。
- 网络报文、UI 截图：N/A，本任务是治理规则调整。
