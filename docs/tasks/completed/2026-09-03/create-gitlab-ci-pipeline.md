# TASK-20260903-011：建立 GitLab CI 验证与 Windows 构建流程

- 任务 ID：TASK-20260903-011
- 状态：已完成
- 任务日期：2026-09-03
- 创建时间：2026-09-03 16:41:57 +08:00
- 开始时间：2026-09-03 16:41:57 +08:00
- 最后更新时间：2026-09-03 17:10:00 +08:00
- 完成时间：2026-09-03 17:10:00 +08:00
- 创建路径：`docs/tasks/pending/2026-09-03/create-gitlab-ci-pipeline.md`
- 归档路径：`docs/tasks/completed/2026-09-03/create-gitlab-ci-pipeline.md`
- 关键词：`GitLab CI`、`.gitlab-ci.yml`、`Deno 2.9.6`、`Rust 1.98.0`、`Android Companion`、`Windows`、`slave4`、`docker test`
- 任务优先级：高
- 优先级理由：CI 会跨 Linux/Windows Runner 执行 Android、前端、Rust、覆盖率和桌面构建门禁，并产出可交付 Windows 文件；错误流程会造成错误业务交付或遗漏受影响平台验证。

## 背景、目标与历史连续性

用户已经在 `http://172.16.3.60/other/intercept_proxy.git` 建立 GitLab 仓库，并要求创建 GitLab CI。
本次只读查询确认 GitLab 为 17.4.2，项目 ID 为 1023、默认分支为 `master`，仓库当前仅有
`README.md`。项目可用 Runner 包含 Windows amd64 `slave1`、`slave2`、`slave4` 和 Linux Runner；
其中 `slave4` 使用较新的 GitLab Runner 17.3.1，Linux `docker test` Runner 用于容器化验证。

历史连续性：

- 当前 GitHub Code verification 已固定 Deno 2.9.6、Rust 1.98.0，先构建并验证 Android Companion，
  再执行覆盖率和桌面验证。
- Windows Rust 1.98 冷缓存验证曾超过 90 分钟，因此完整 Windows verify 预算保持 150 分钟。
- 现有正式签名发布只在 GitHub workflow 中具备三项 Authenticode fail-closed 合同；本次不迁移或
  推定 GitLab 签名密钥。

## 范围、不在范围与需求确认

范围：

- 新增根目录 `.gitlab-ci.yml`，在 Merge Request、默认分支、Tag 和 Web 手动流水线运行。
- Linux Runner 构建并验证 Android Companion，保存固定名称 APK artifact。
- Linux Runner 执行前端与 Rust 覆盖率门禁，并保存 coverage artifact。
- Windows `slave4` Runner 执行生成绑定、前端、架构、Rust、协议包及独立 runtime gates。
- 默认分支、Tag 和 Web 流水线在完整验证通过后生成明确未签名的 MSI、NSIS 和 portable ZIP。
- CI 中继续固定 Deno 2.9.6、Rust 1.98.0、Gradle 9.6.1、Android API 36、Build Tools 36.0.0 和
  NDK 29.0.14206865；GitLab job 不调用 Node/npm/pnpm。
- 增加 GitLab CI 合同测试，并让现有 Deno/Rust 工具链合同覆盖 `.gitlab-ci.yml`。
- 更新开发文档，说明 GitHub 与 GitLab CI 的职责和 GitLab 未签名产物边界。

不在范围：

- 不修改产品源码、协议、UI、数据库或运行时行为。
- 不迁移 macOS workflow；当前 GitLab 没有 macOS Runner。
- 不配置或导入 Authenticode 证书，不创建签名发布或 GitLab Release。
- 不添加部署、自动合并、自动发布或 CI 失败重试/忽略策略。
- 不推送当前工作树到 GitLab，不触发远程流水线；远程执行必须由用户另行明确授权。
- 不覆盖当前工作区其他任务的未提交修改。

需求确认记录：

- 用户要求“创建 gitlab CI 流程”，随后提供已建立的 GitLab 仓库地址。
- CI 验证层级从当前 `.github/workflows/ci.yml`、仓库需求和发布文档取得，不新增产品行为。
- “CI”解释为代码验证加明确未签名 Windows 产物；签名、发布和部署属于不同权限及密钥边界，保持不在范围。

## 需求就绪检查

- 问题、目标和成功结果：PASS；GitLab 能解析配置，既有验证链在对应 Runner 上有明确 job。
- 范围与不在范围：PASS。
- 输入、输出和状态变化：PASS；输入为 Git commit/MR/tag/web pipeline，输出为门禁结果、Companion APK、coverage 和未签名 Windows artifacts；不改变外部状态。
- 具体示例：PASS；Merge Request 执行 Android、coverage、Windows verify，不执行 installer；`master` push 在 verify 后执行 unsigned package。
- 可判断验收标准：PASS，见下节。
- 会改变实现方向的未确认事项：零；Runner/tag/版本已通过当前 GitLab API 与仓库合同确认。
- 进入实现时间：2026-09-03 16:41:57 +08:00。

## 现状、事实、推断与未知

- 当前已验证：GitLab 17.4.2、项目 1023、默认分支 `master`、项目私有、CI/CD 已启用。
- 当前已验证：`slave4` 为 online Windows amd64 Runner，标签 `slave4`；`docker test` 为 online Linux amd64 Runner。
- 当前已验证：远端 Git 仓库可读取，`master` 当前只含 `README.md`。
- 当前已验证：本地分支相对 GitHub origin ahead 5，且存在多项其他任务的未提交修改。
- 推断：Linux Runner 的 `docker test` 标签表示可执行带 `image` 的容器 job；本次只通过 GitLab CI Lint
  验证配置，未触发 Runner，远程 executor 能力保留为执行期验证项。
- 未知：Runner 对 Docker Hub/GitHub/Android Maven 的实时网络可达性、Windows Visual Studio/Tauri
  构建依赖现状；首次远程流水线前不把这些环境条件记为 PASS。

## 最小改动与最优设计

| 方案 | 分析 |
| --- | --- |
| 最小改动 | 只增加一个 Windows quick executable job，文件少，但跳过 Android、覆盖率、架构、Rust 完整门禁，不能与当前 Code verification 合同等价，容易把“能编译”误报为“可交付”。 |
| 最优设计 | 用 prepare、quality、verify、package 四阶段表达依赖；Companion 和 coverage 在 Linux 容器执行，Windows 平台门禁与打包由 `slave4` 执行；复用现有脚本和版本合同，installer 明确 unsigned。 |

采用最优设计；不增加产品依赖或签名路径，使用 GitLab 原生 `needs`、artifacts、cache、rules 和 job timeout。

## 验收标准

1. GitLab 17.4 CI Lint 对 `.gitlab-ci.yml` 返回 valid，pipeline workflow 仅覆盖 MR、默认分支、Tag 和 Web。
2. Android Companion 是 coverage、Windows verify 和 package 的显式上游，APK 通过 artifact 传递，不使用占位资源。
3. coverage 执行现有 coverage-policy、frontend、Rust coverage 三层门禁并保存实际 coverage 目录。
4. Windows verify 保持 150 分钟预算，执行绑定确定性、前端/架构、Rust fmt/clippy/test、协议包和独立 runtime gates。
5. package 只在非 MR 的默认分支、Tag 或 Web 流水线运行，生成 MSI、NSIS 和 portable ZIP，并逐项证明未签名。
6. GitLab CI 固定 Deno 2.9.6 和 Rust 1.98.0；所有前端安装/任务只使用 Deno，不出现 Node/npm/pnpm 调用。
7. CI 合同测试、Deno 工具链合同测试、YAML 解析、格式检查和 `git diff --check` 通过。
8. 远程 Runner 执行未获授权时记录 `NOT_RUN`，不得用 CI Lint 或本地测试替代。

## 小任务、测试、文档与审查

| ID | 内容 | 状态 | 验收 |
| --- | --- | --- | --- |
| GLCI-01 | 建立 GitLab workflow、Runner 和 artifact 合同测试 | 已完成 | 测试识别触发、依赖、版本、超时和 unsigned 边界，4/4 PASS |
| GLCI-02 | 实现 `.gitlab-ci.yml` 与可复用环境准备脚本 | 已完成 | GitLab CI Lint valid，Linux Shell 与 YAML 静态检查通过 |
| GLCI-03 | 同步工具链合同与开发文档 | 已完成 | Deno/Rust 合同 4/4 PASS，onboarding 已说明 GitHub/GitLab 职责 |
| GLCI-04 | 定向验证、证据、整体对抗审查和归档 | 已完成 | 独立复审 P0/P1/P2=0；远程 pipeline 明确 NOT_RUN |

测试计划：先增加 GitLab workflow 合同测试并确认缺少配置时 RED；实现后运行 Deno 测试、YAML
解析、Shell/PowerShell 语法检查、GitLab 项目 CI Lint 和 `git diff --check`。不运行完整产品测试，因为本次
只改变 CI/脚本/文档；远程 runner 行为必须以后续授权的真实 pipeline 作为正式运行证据。

文档影响：更新 `docs/onboarding-guide.md` 的 Git/CI 章节；创建本任务证据目录与完成索引。

对抗审查：重点检查重复 pipeline、Runner 标签、跨 job artifact、cache 安全、版本固定、shell 差异、
MR/package 触发边界、unsigned 证明、密钥泄露和“静态 valid 被误报为远程 PASS”。

## 实施、测试与完成总结

实施记录：

- 新增 `.gitlab-ci.yml`，用 `prepare -> quality -> verify -> package` 表达四阶段依赖；workflow 只接受
  Merge Request、push 默认分支、push Tag 和 Web，其他 source fail-closed 为 `never`。
- Linux `docker test` Runner 构建 Android Companion 并运行 coverage；Windows `slave4` Runner
  消费 APK artifact，执行完整验证并在允许的 pipeline source 上输出明确 unsigned 的 MSI、NSIS 和 ZIP。
- 新增 Linux/Windows 环境准备脚本；Deno、rustup、Gradle 和 Android command-line tools 固定版本和
  SHA-256，下载内容在解压或执行前 fail-closed 校验。
- 增加 `test:gitlab-ci` 并把 GitLab workflow 纳入 Deno/Rust 工具链合同；更新 `.gitignore` 和
  `docs/onboarding-guide.md`。

修改文件：

- `.gitlab-ci.yml`
- `.gitignore`
- `package.json`
- `scripts/ci/bootstrap-gitlab-linux.sh`
- `scripts/ci/bootstrap-gitlab-windows.ps1`
- `scripts/ci/stage-android-companion-windows.ps1`
- `scripts/gitlab-ci-contract.test.mjs`
- `scripts/deno-toolchain-contract.test.mjs`
- `docs/onboarding-guide.md`
- 本任务文档、任务索引和测试证据索引

验收与测试：

- `deno task test:gitlab-ci`：PASS，4/4。
- `deno task test:deno-toolchain`：PASS，4/4。
- `deno fmt --check`、`deno lint`、定向 ESLint、`bash -n`、Ruby YAML parse、`git diff --check`：PASS。
- GitLab project `1023` CI Lint：PASS，`valid=true`、`errors=[]`、`warnings=[]`，解析出四个预期 job。
- 独立对抗复审：APPROVE，P0/P1/P2=0；确认 workflow source、固定下载 SHA、合同测试位置和断言
  作用域三个初审 P2 均已修复。
- 本机无 `pwsh`，PowerShell 脚本只完成静态复审；未授权推送或触发 GitLab pipeline，因此真实 Runner、
  APK、coverage、MSI、NSIS、ZIP 和 Authenticode 状态均为 `NOT_RUN`。

测试证据：

- [gitlab-ci-pipeline](../../../testing/evidence/2026-09-03/TASK-20260903-011/gitlab-ci-pipeline/README.md)

完成总结：`PASS_WITH_REMOTE_PIPELINE_NOT_RUN`。GitLab CI 配置、静态合同、服务器端 CI Lint 和独立
复审均通过；远程 pipeline 是单独的外部执行门禁，不以本次静态 PASS 替代。
