# ADR-003：统一 application ZIP 的所有权与版本

- Status: Accepted and implemented
- Implementation owner: R07a-R07e
- Date: 2026-08-17
- Scope: application-wide backup/import archive, not protocol-package author ZIP

## Context

仓库已有协议包 author ZIP；应用数据的完整备份/恢复需要另一套受控 archive，不能由各 adapter 或页面
分别定义 ZIP、版本和迁移，否则会产生无法一致预览、回滚或升级的多套 wire。

## Decision

application crate 唯一拥有 archive manifest、format version、strict wire、迁移顺序、引用完整性和
prepare/commit 语义。首个落地格式命名为 application archive v1。infrastructure 只实现 native dialog、
ZIP 安全限额、临时文件、flush/fsync/atomic rename、SQLite transaction 和失败补偿。WebView 只接收无敏感
内容的 preview 和一次性 token。

协议包 author ZIP 仍由 `protocol-scripting` 读取和校验；它是 application archive 中的一类受控 payload，
不是 application archive 本身。

## Alternatives

- Rejected：页面拥有 ZIP。它会暴露路径/字节并复制 Rust 业务校验。
- Rejected：infrastructure 拥有版本/wire。它会把业务迁移和引用规则放进 I/O adapter。
- Rejected：Workspace、settings、cert、package 各自独立 ZIP。它无法提供一致快照和原子恢复。
- Accepted：application-owned v1 wire + infrastructure-owned atomic I/O。

## Consequences

- application archive v1 已由 application 严格 wire、prepare/commit/discard 用例和 infrastructure 安全
  reader/writer 实现；原生文件对话框只负责选择路径，不拥有业务格式。
- 真实 archive reader/writer 已覆盖 ZIP 限额、一次性 preview token、revision/running guard、原子 commit、
  失败补偿和确定性导出。
- 格式升级必须显式版本化并 fail-closed 拒绝 unknown field；不得用 `PRAGMA user_version` 代替 application wire version。

## Open items

- future archive version 必须新增 ADR、严格 wire 和显式迁移策略；1.0 不读取旧 JSON 配置或旧 archive。
- Windows/macOS 打包后的原生打开、保存与启动 smoke 仍由发布验证持续覆盖。
