# DOC-GOV-006 复测步骤

从仓库根目录执行以下检查：

```bash
set -eu

before_agents=$(stat -f '%z:%m' AGENTS.md)
before_index=$(stat -f '%z:%m' docs/testing/quick-validations/README.md)

test "$(rg -n '^### 10\.5 快速配置验证$' AGENTS.md | wc -l | tr -d ' ')" = 1

for branch in '已有正式任务负责同一验证' '尚无正式任务且任一进入条件不满足' 'QV 执行中触发升级条件'; do
  rg -q "$branch" AGENTS.md
done
rg -q '不评定高、低优先级' AGENTS.md

for lifecycle in RESERVED RUNNING FINAL; do
  rg -q "$lifecycle" AGENTS.md
  rg -q "$lifecycle" docs/testing/quick-validations/README.md
  rg -q "$lifecycle" docs/testing/quick-validations/TEMPLATE.md
done
rg -q '验证结果在生命周期进入 `FINAL` 前必须为 `null`' AGENTS.md
rg -q 'FINAL.*终态' AGENTS.md
tr '\n' ' ' < AGENTS.md | rg -q '第一个成功写入 `FINAL` 的 +所有者获胜.*不得重复写入'

rg -q 'session_id.*process_id.*started_at' AGENTS.md
rg -q '不得仅按经过时间自动接管' AGENTS.md
rg -q '结果记为 `INCONCLUSIVE`' AGENTS.md
rg -q '仍在变化时保持记录为 `RUNNING`' AGENTS.md
rg -q '恢复必须拆成三个事务' AGENTS.md
rg -q '禁止持锁执行网络、证书、运行时审计或清理' AGENTS.md
tr '\n' ' ' < AGENTS.md | rg -q '把 `metadata.json.runner` 与索引中的 +执行所有者原子更新为恢复者'
tr '\n' ' ' < AGENTS.md | rg -q '原执行者在任何后续写入前.*发现所有权.*立即停止'

for phrase in '资源解析' 'DNS' 'TCP' '直接 TLS' 'Proxy 上游测试' '下游 Listener' '完整 Proxy 链路' '应用协议'; do
  rg -q "$phrase" AGENTS.md
  rg -q "$phrase" docs/testing/quick-validations/TEMPLATE.md
done
for state in VERIFIED FAILED INCONCLUSIVE NOT_RUN 'N/A'; do
  rg -q "$state" AGENTS.md
  rg -q "$state" docs/testing/quick-validations/TEMPLATE.md
done
rg -q 'path_id' AGENTS.md docs/testing/quick-validations/TEMPLATE.md
rg -q 'Listener 接受该连接的会话或连接标识' AGENTS.md
rg -q '不能把时间接近的' AGENTS.md

rg -q '传播顺序为 `FAILED` 高于 `INCONCLUSIVE`' AGENTS.md
rg -q '适用清理不是 `VERIFIED` 时整体结果不得为 `VERIFIED`' AGENTS.md
rg -q '清理结论：`NOT_RUN | VERIFIED | FAILED | INCONCLUSIVE | N/A`' \
  docs/testing/quick-validations/TEMPLATE.md

for field in '操作系统和架构' 'Proxy 构建版本' '测试工具、运行时和版本' \
  'DNS 配置和当次解析上下文' '系统代理、VPN 或透明路由状态' '外部服务或硬件'; do
  rg -q "$field" docs/testing/quick-validations/TEMPLATE.md
done
rg -q '只写“当前草稿”' AGENTS.md

tr '\n' ' ' < AGENTS.md | rg -q '不自动授权应用报文、交易或其他.*外部状态操作'
rg -q '快速验证本身不触发 CI' AGENTS.md
rg -q '登记正式任务不扩大外部操作权限' AGENTS.md
tr '\n' ' ' < AGENTS.md | rg -q '正式任务在自己的证据目录重新执行'

rg -q '原始资源快照属于验证档案，不属于产品运行状态' AGENTS.md
tr '\n' ' ' < AGENTS.md | rg -q '不得修改已经.*完成的 QV 档案内容'

test -f docs/testing/quick-validations/README.md
test -f docs/testing/quick-validations/TEMPLATE.md
test "$(rg -n 'testing/quick-validations/README.md' docs/README.md | wc -l | tr -d ' ')" = 1
awk '/^```json$/{flag=1;next} /^```$/{if(flag){exit}} flag' \
  docs/testing/quick-validations/TEMPLATE.md | jq -e . >/dev/null

zsh docs/testing/evidence/2026-08-26/TASK-20260826-002/DOC-GOV-006/replay/validate-state-scenarios.zsh
rg -q 'real concurrent allocation' \
  docs/testing/evidence/2026-08-26/TASK-20260826-002/DOC-GOV-006/outputs/state-scenarios.txt
rg -q 'recovery ownership persisted, stale runner rejected' \
  docs/testing/evidence/2026-08-26/TASK-20260826-002/DOC-GOV-006/outputs/state-scenarios.txt
rg -q 'two-finalizer race: REJECTED SUCCESS' \
  docs/testing/evidence/2026-08-26/TASK-20260826-002/DOC-GOV-006/outputs/state-scenarios.txt

after_agents=$(stat -f '%z:%m' AGENTS.md)
after_index=$(stat -f '%z:%m' docs/testing/quick-validations/README.md)
test "$before_agents" = "$after_agents"
test "$before_index" = "$after_index"
```

所有命令成功后，本用例结果为 PASS。任何检查失败时保留失败位置和当前权威内容，不得通过删除要求或
放宽预期结果获得 PASS。
