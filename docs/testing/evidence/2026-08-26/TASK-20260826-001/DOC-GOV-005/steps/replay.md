# 复测步骤

从仓库根目录执行：

```bash
set -e
rg -n '^### 4\.1 需求就绪门禁$|^### 4\.2 问题与根因分析$|^### 5\.3 任务优先级与流程等级$|^### 5\.4 单工作区全局任务锁$|最低验证要求如下|统一锁工具落地前' AGENTS.md
rg -n '高优先级执行完整流程|低优先级执行必要流程|必须停止实现并重新确认优先级|高优先级任务必须进行一次整体对抗审查' AGENTS.md
test -f docs/tasks/completed/2026-08-26/agent-workflow-governance-optimization.md
test "$(rg -c 'agent-workflow-governance-optimization\.md' docs/README.md || true)" -eq 0
test "$(rg -c 'agent-workflow-governance-optimization\.md' docs/tasks/README.md)" -eq 1
rg -n '具体示例：.*公共接口说明与实现合同冲突' docs/tasks/completed/2026-08-26/agent-workflow-governance-optimization.md
if rg -n '[[:blank:]]+$' AGENTS.md docs/README.md docs/tasks/README.md docs/tasks/completed/2026-08-26/agent-workflow-governance-optimization.md; then
  exit 1
fi
```

所有命令退出成功即为 PASS。
