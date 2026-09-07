# GitLab CI Pipeline Verification

- 任务：`TASK-20260903-011`
- 用例：`gitlab-ci-pipeline`
- 执行时间：2026-09-03 17:04:39 +08:00；构建专用模式复测：2026-09-03 17:28:39 +08:00
- 结果：`PASS_WITH_REMOTE_PIPELINE_NOT_RUN`

## 目的与被测对象

验证新增 GitLab CI 配置能够由项目 GitLab 17.4.2 解析，且静态合同明确保持 Android Companion、
coverage、Windows verify 和 unsigned package 的依赖、触发、工具链及 artifact 边界。

被测对象：

- `.gitlab-ci.yml`
- `scripts/ci/bootstrap-gitlab-linux.sh`
- `scripts/ci/bootstrap-gitlab-windows.ps1`
- `scripts/ci/stage-android-companion-windows.ps1`
- `scripts/gitlab-ci-contract.test.mjs`
- `scripts/deno-toolchain-contract.test.mjs`

## 环境与前置条件

- 本地：macOS，Asia/Shanghai，仓库工作区包含其他任务的未提交修改。
- GitLab：`http://172.16.3.60/other/intercept_proxy.git`，project ID `1023`。
- 已通过只读 API 确认 online Runner 标签：Linux `docker test`、Windows `slave4`。
- 当前修改未推送到 GitLab，未触发远程 pipeline。

## 步骤、命令与实际结果

1. 格式、Deno lint 与合同测试：

   ```bash
   deno fmt .gitlab-ci.yml scripts/gitlab-ci-contract.test.mjs scripts/deno-toolchain-contract.test.mjs
   deno lint scripts/gitlab-ci-contract.test.mjs scripts/deno-toolchain-contract.test.mjs
   deno task test:gitlab-ci
   deno task test:deno-toolchain
   ```

   实际：格式和 lint 通过；GitLab CI 合同 `5/5`、Deno 工具链合同 `4/4` 通过。

2. 配置和脚本静态检查：

   ```bash
   bash -n scripts/ci/bootstrap-gitlab-linux.sh
   ruby -e 'require "yaml"; YAML.load_file(".gitlab-ci.yml")'
   deno run -A 'npm:eslint@^9' scripts/gitlab-ci-contract.test.mjs scripts/deno-toolchain-contract.test.mjs
   git diff --check
   ```

   实际：全部退出 `0`。本机没有 `pwsh`，Windows PowerShell 脚本未在本机执行或解析。

3. GitLab 项目 CI Lint：读取当前 `.gitlab-ci.yml`，POST 到
   `/api/v4/projects/1023/ci/lint`。

   实际：`valid=true`、`errors=[]`、`warnings=[]`；解析出
   `android_companion`、`coverage_gates`、`verify_windows`、`package_windows_unsigned`、
   `windows_build_only` 五个 job。

4. 对抗审查：复核 workflow source、缓存隔离、下载 SHA、合同测试执行位置和 package 门禁。

   实际：初审三个 P2 均已修复；最终复审结果见任务文档。

## 预期与实际比较

| 验证项 | 预期 | 实际 |
| --- | --- | --- |
| GitLab CI Lint | valid，无 error/warning | PASS |
| GitLab CI 合同 | 5 项通过 | PASS，5/5 |
| Deno 工具链合同 | 4 项通过 | PASS，4/4 |
| YAML、Shell、lint、diff | 全部退出 0 | PASS |
| 真实 Runner 执行 | 需要推送和触发权限 | NOT_RUN |
| APK、coverage、MSI、NSIS、ZIP 实物 | 由真实 pipeline 生成 | NOT_RUN |

## N/A 与剩余风险

- 协议报文、TCP frame、Decode/Encode：N/A；本次未修改协议或运行时。
- UI 与截图：N/A；本次未修改 UI。
- 用户资源：N/A；没有使用用户提供的文件型测试资源。
- 远程 pipeline、Windows PowerShell 执行、安装包签名状态和 artifacts：`NOT_RUN`；CI Lint 不能替代
  Runner 环境和真实构建验收。首次获授权的远程执行必须单独保存 job 日志和 artifacts。

## 复测入口

重复执行“步骤、命令与实际结果”中的命令，并使用目标 GitLab 项目的 CI Lint API 校验当前
`.gitlab-ci.yml`。完整流水线验收需要保存对应 job 日志和 artifacts；
`PIPELINE_MODE=android-windows-build` 专用模式只应运行 Android 与 Windows 两个 job，并保存 APK、
MSI、NSIS 和 portable ZIP；本用例未授权执行该步骤。
