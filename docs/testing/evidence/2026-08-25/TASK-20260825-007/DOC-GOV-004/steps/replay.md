# 复测步骤

1. 从仓库根目录检查 `AGENTS.md` 的“单工作区全局任务锁”章节。
2. 检查锁目录、原子获取、持锁范围、禁止持锁执行耗时工作和遗留锁处理规则。
3. 扫描 pending、completed 与测试证据目录中的任务 ID，确认主任务 ID 唯一且证据任务均存在。
4. 检查任务处于 pending 时 README 入口恰好一个，完成后入口为零且完成索引恰好一个。
5. 在隔离临时目录执行 `replay/lock-protocol.sh`，验证等待重试、错误所有者释放拒绝、部分写入失败、
   有效/缺失/损坏原所有者恢复、损坏原文逐字节保留，以及恢复者在创建恢复主锁前后失活时的再次接管。
6. 验证恢复主锁的有效、缺失和损坏所有者状态；连续两代恢复者失活时，每代中断目录唯一、历史目录
   保留并与 `interrupted_recovery_locks` 一一对应。
7. 验证 `recovered_from` 不匹配、存在多个外层隔离目录和中断目录目标冲突时均 fail-closed。

```bash
rg -n \
  '单工作区全局任务锁|\.task-manager\.lock|原子 `mkdir|必须使用同一把锁|不得在持锁期间执行耗时|original_owner_state|interrupted_recovery_locks|主锁状态变化|并行批次' \
  AGENTS.md

primary_ids=$(rg --no-filename \
  '^- 任务 ID：TASK-[0-9]{8}-[0-9]{3}$' \
  docs/tasks/pending docs/tasks/completed | sed 's/^- 任务 ID：//' | sort)
test -z "$(printf '%s\n' "$primary_ids" | uniq -d)"

evidence_ids=$(find docs/testing/evidence -type d -name 'TASK-[0-9]*' -exec basename {} \; | sort -u)
printf '%s\n' "$evidence_ids" | while IFS= read -r task_id; do
  test -z "$task_id" || printf '%s\n' "$primary_ids" | rg -qx "$task_id"
done

pending_path=docs/tasks/pending/2026-08-25/task-management-global-lock.md
completed_path=docs/tasks/completed/2026-08-25/task-management-global-lock.md
pending_entry_count=$(rg -c 'task-management-global-lock\.md' docs/README.md || true)
completed_entry_count=$(rg -c 'task-management-global-lock\.md' docs/tasks/README.md || true)
pending_entry_count=${pending_entry_count:-0}
completed_entry_count=${completed_entry_count:-0}
if test -f "$pending_path"; then
  test "$pending_entry_count" -eq 1
  test "$completed_entry_count" -eq 0
else
  test -f "$completed_path"
  test "$pending_entry_count" -eq 0
  test "$completed_entry_count" -eq 1
fi

bash docs/testing/evidence/2026-08-25/TASK-20260825-007/DOC-GOV-004/replay/lock-protocol.sh
```
