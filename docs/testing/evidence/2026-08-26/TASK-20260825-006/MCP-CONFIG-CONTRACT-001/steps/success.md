# 成功步骤与判定

1. Application 报告 211、14、7、5、12 五组全部通过。
2. MCP 报告 31 项通过。
3. strict Clippy、fmt、architecture、source-size 和 diff-check 全部通过。
4. 七个归档 fixture 与活动 fixture SHA-256 一一相同。
5. 精确 patch 有 29 个唯一 header，与 29 路径白名单完全一致。
6. `git apply --reverse --check` 成功。
7. 独立审查为 `APPROVE`，P0/P1/P2 为 0/0/0。

任一适用项失败则本用例不得标记 PASS。
