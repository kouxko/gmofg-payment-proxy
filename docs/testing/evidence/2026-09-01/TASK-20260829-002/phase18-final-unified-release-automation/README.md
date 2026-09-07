# Phase18 最终统一规则与 Universal 发布自动化证据

## 结论

- 自动化范围：`PASS`。
- 独立审查：Reviewer `APPROVED`、Verifier `VERIFIED`，P0/P1/P2=`0/0/0`，`code_checkpoint_ready=true`。
- TASK 状态：`进行中`。需要人为干预的GUI、系统/网络权限、Windows、Android和Developer ID验收按用户要求留到明日，不以本次自动化结果替代。
- 用户已取消hash验收；本证据不生成或声明源码、worktree或artifact hash。

## 被测对象与范围

- 基线HEAD：`30d0f69e0e2fed745fbee2a1dc49271517e76d41`，验收对象为其上的Phase18工作区变更。
- 核心范围：递归Document与统一匹配、规则UI、单一RuleDefinition、listener-scoped RuntimeRuleBundle、HTTP/Socket generation lease、Application/Infrastructure/Domain/Tauri调用方、环境wire、bindings、checkers、macOS Universal打包脚本及Windows workflow Sidecar staging。
- 明确排除且未修改/未暂存：`docs/README.md`用户剩余两条任务入口、`docs/tasks/pending/2026-08-31/`无关任务、外部CI/发布状态。
- 完整逻辑范围见 `resources/file-scope.txt`。

## 关键实现结果

- HTTP条件统一为Method、request target `/path?query`、Header；Document条件支持递归RFC6901、Schema下拉/手工输入、无Schema手工输入和wildcard。
- UI不再提供field/operator/action/Nth默认选择；root `""`与empty-name property `"/"`独立表达；显示阶段为`Proxy → Server`和`Proxy → App`。
- 单一RuleDefinition物理替代旧Rule/RuleEngine/RuleDraft和旧投影/CRUD/wire；未增加alias、兼容、fallback或双路径。
- RuntimeRuleBundle把workspace revision、RuleDefinitions与HTTP/Socket compiled programs绑定为同一listener generation；保存、CAS和publish由同一transaction gate保护。
- HTTP keep-alive与Socket长连接每个消息/frame获取当前generation；停止/启动/token切换及Socket读program前取锁竞态均由确定性RED→GREEN覆盖。
- unknown rule id fail-closed；Encode或生命周期提交失败回滚working Exchange、Nth/one-shot和lifecycle。

## 自动化验证

- Frontend：67 files / 571 tests，全部PASS。
- Domain：71+13+9+8=`101`个有效测试PASS。
- Application：427+15+7+5+5+6+12=`477`个有效测试PASS。
- Infrastructure：473+7+24+7+8=`519`个有效测试PASS。
- Host：10+4+13+1+3=`31`个有效测试PASS。
- Package runtime：2+6+3+9+2=`22`个有效测试PASS。
- 0-test binary/target未计入成功证据。
- Workspace check、strict Clippy `-D warnings`、Rust fmt、source-size、diff-check全部PASS。
- Bindings fresh/deterministic，mutation 6/6 PASS。
- Architecture docs9/ADR9/MCP5、boundary76+8、runtime43/owned10+10/debt0、Socket24、frontend boundary均PASS。
- Phase18 checker17/17；受影响checker30/30；最终checkers116/116 PASS。
- 最终生产与generated中旧`ProtocolDocument*`、`RuleEngine`、`RuleDraft`、`from_http_conditions`零命中；新增diff中alias/default/fallback/compat零命中。

## Universal App/DMG

- 命令：`pnpm build:macos:universal`。
- 会话：`3171`，exit `0`。
- App：`src-tauri/target/universal-apple-darwin/release/bundle/macos/Intercept Proxy.app`。
- DMG：`src-tauri/target/universal-apple-darwin/release/bundle/dmg/Intercept Proxy_1.0.0_universal.dmg`。
- 主程序：111655488 bytes，`x86_64 arm64`。
- Sidecar：29443472 bytes，`x86_64 arm64`。
- DMG：59666432 bytes。
- Bundle identifier：`com.interceptproxy.desktop`。
- App bundle和挂载DMG内App均通过architecture、Info.plist和`codesign --verify --deep --strict`校验；DMG只读挂载后成功detach。
- App/Sidecar测试进程已退出，8765/17653端口已释放，无孤儿进程。
- session原始stdout未持久化且无法恢复；本证据只记录已捕获的命令、exit、artifact状态和独立复验结果，不伪造输出。

## RED 与修复边界

- 旧连接热替换：HTTP与Socket旧capability先真实FAIL；改为共享generation handle后PASS。
- 原子发布：runtime失败曾导致revision 1→3；改为candidate完整compile、listener gate内单次CAS+publish后失败不持久化。
- Listener状态竞态：Stopped→Running真实RED错误保存；完整`Stopped|Running(run_token)`基线CAS后PASS。
- Socket读锁顺序：旧program可越过gate的barrier测试先FAIL；先lock再读program后PASS。
- 旧规则模型与legacy helper：物理删除并由mutation checker防回流。
- Universal打包：补全Universal Sidecar、可复现vendored OpenSSL cross build、bundle-level ad-hoc sealing和从已签名App创建DMG；最终canonical命令PASS。

## 明日人工验收

- `NOT_RUN`：GUI/computer-use规则创建、条件树、Document树、会话/抓包显示与截图。
- `NOT_RUN`：macOS网络/系统权限弹窗与真实权限选择。
- `NOT_RUN`：Windows真实运行/CI、Android设备或模拟器。
- `NOT_RUN`：Developer ID正式签名、公证与Gatekeeper分发验证。
- `NOT_RUN`：push、外部CI、release。

复测命令见 `replay/commands.md`；结构化结果见 `outputs/verification-summary.json`。
