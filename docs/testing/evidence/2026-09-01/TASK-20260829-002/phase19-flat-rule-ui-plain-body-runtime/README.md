# Phase 19：扁平规则 UI 与 Plain Body 真实运行验收

## 目的

验证需求变更后的当前合同：规则使用非空扁平 `conditions[]` 且固定 AND；规则编辑器固定在页面右侧；上下行规则统一列表并用方向标识区分；Plain HTTP Body 可直接用 RFC 6901 路径创建条件；界面中的条件输入称为“匹配值”、动作输入称为“动作值”，不把领域值描述为 JSON；多行“动作参数 JSON”独占一行；Document 根路径不提供独立按钮，路径输入框中的 `/` 直接表示根路径。

## 被测对象与环境

- App：`src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Intercept Proxy.app`
- 架构：Apple Silicon arm64。
- 配置：隔离 `HOME/CFFIXED_USER_HOME=/tmp/gmofg-rule-round2.58al97/home`，未读取或修改用户正式数据库。
- Listener：`127.0.0.1:8080`，Plain HTTP Body，无协议包。
- 本地 Server：`127.0.0.1:18083`。
- 规则：`Body age 18`，上行，`/customer/age` number equals `18`，动作 `RecordMatch`。

## 实际步骤与结果

1. 通过实际 App UI 创建并保存规则，卡片显示“上行”；规则列表不再按 `Proxy → Server`/`Proxy → App` 分区。
2. 发送 `match.json`：Server 返回 200，运行日志 `log_id=7/35` 均记录 `matched=true` 和 `record_match`。
3. 发送 `miss.json`：Server 返回 200，运行日志 `log_id=18` 记录 `matched=false`、无动作。
4. 发送 `invalid.txt`：运行日志 `log_id=27` 在 `decode` 阶段返回 `JSON_INVALID`，Exchange 失败关闭；没有透传或回退。
5. 实际 App 可访问性树确认 `匹配值=true`、`动作值=true`、`JSON 值=false`；截图见 `outputs/rule-editor-text-labels.jpeg`。
6. 选择 `Jitter` 后，`HTTP 动作类型`与`创建 HTTP 动作`同一行，`动作参数 JSON`独占下一整行；截图见 `outputs/http-action-parameters-own-row.jpeg`。
7. 物理删除“手动选择根路径 /”按钮；实际 App 中只保留路径输入框，直接输入 `/`。组件回归确认该显示值传给 Rust 工厂时转换为内部根路径空字符串；Schema 中真实空名称属性 `/` 仍保持原路径。截图见 `outputs/document-root-direct-input.png`。
8. 增量重建 arm64 `.app`，执行 bundle-level ad-hoc sealing；`codesign --verify --deep --strict --verbose=2` 通过。
9. 退出 App、Sidecar 与本地 Server 后，8080、8765、17653、18083 均无监听，无孤儿进程。

## 自动化证据

- RED：focused UI test 因缺少 `http-action-controls` 失败 1/4，证明旧布局仍将多行参数混入两列 auto-flow。
- RED：新增根路径回归后，Rust 工厂实际收到 `/` 而非内部根路径 `""`，focused test 1/5 FAIL。
- GREEN：`pnpm exec vitest run src/features/rules`，4 files / 19 tests PASS；根路径用例证明按钮不存在且手工 `/` 映射为内部根路径。
- `pnpm typecheck`、`pnpm exec eslint src/features/rules --max-warnings=0`、`pnpm scan:source-size`、`git diff --check` 均 PASS。
- 本次最终布局改动前的同轮完整门禁：前端 64 files / 534 tests、Phase15/16/统一模型 Node 31/31、workspace check、strict Clippy、fmt、lint、typecheck、source-size、diff 全部 PASS；最终布局仅修改前端组件和对应测试，随后以上 focused/static 门重新通过。
- 独立最终审查：P0/P1/P2=`0/0/0`，未发现代码、规格或安全阻断。

## 复测

见 `steps/replay.md`。输入、运行读回和 UI 状态分别保存在 `inputs/` 与 `outputs/`。

## N/A / NOT_RUN

- 外部网络、系统权限弹窗、Windows、Android、Developer ID 正式签名/公证：`NOT_RUN`，本用例不触发。
- CI、push、release：`NOT_RUN`，未获授权。
- Hash：用户已取消 hash 验收，本用例不生成或声明 hash。
