# RULE-FORM-IMMEDIATE-001

- 目的：验证新建规则在名称和优先级为空时仍立即显示条件与动作表单，同时禁止不完整保存。
- 被测基线：`3586ea2`
- 执行时间：2026-09-03 16:15:10 ～ 16:20:01 +08:00
- 环境：macOS arm64；Deno/Vitest；Tauri 2；本机 `/Applications/Intercept Proxy.app`。

## 步骤与结果

1. 执行 `deno task test src/features/rules/rules-view.test.tsx src/features/rules/rule-definition-editor.test.tsx`。
   - 实际：2 个测试文件、14/14 PASS。
   - 覆盖：空名称/优先级时完整表单可见、保存禁用、既有条件/动作物化路径。
2. 执行 `deno task typecheck`、`deno task lint` 和 `git diff --check`。
   - 实际：全部退出码 0。
3. 执行 `deno task tauri build --bundles app`。
   - 实际：Next.js 13 个静态页面、Rust release 和 macOS `.app` 生成成功。
4. 安装、ad-hoc 签名并启动正式包。
   - 安装路径：`/Applications/Intercept Proxy.app`
   - 旧包备份：`/Users/codin/.Trash/Intercept Proxy.app.codex-old-20260903-161830`
   - 可执行文件 SHA-256：`3bedde95f2977f333529aa78eddc2ce75699f8fda02f5657d7e104352d5fcec4`
   - 运行 PID：`15686`
   - 严格签名校验：PASS。
5. 在已安装 App 打开“拦截规则 → 新建规则”，选择 `Payment DLL · HTTP` 和 `Proxy → Server`，不填写名称或优先级。
   - 实际：名称为空、优先级为空；“HTTP 规则内容”“匹配条件”“对应动作”均可见；“保存规则”禁用。

## 判定

PASS。内容可见性只依赖 Listener/阶段能力；元数据与内容完整性仍独立控制保存。

## 不适用项

- 规则 Runtime、网络报文、A920MAX：N/A；本任务不保存或执行规则，不改变运行时合同。
- CI、push、发布：N/A；用户要求本地修改、安装与提交。
- 对抗审查：N/A；按用户在当前连续交付中的明确要求跳过。

复测命令见 `outputs/test-summary.txt`。
