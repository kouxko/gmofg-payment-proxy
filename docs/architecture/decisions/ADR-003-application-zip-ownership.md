# ADR-003：统一 application ZIP 的所有权与版本

- Status: Accepted design; implementation deferred
- Implementation owner: R07a-R07e
- Date: 2026-08-17
- Scope: application-wide backup/import archive, not protocol-package author ZIP

## Context

仓库已有 Workspace/config 文档和协议包 author ZIP，但 application-wide 原子备份/恢复尚未实现。若每个
adapter 或页面各自写 ZIP、版本和迁移，会产生无法一致预览、回滚或升级的多套 wire。

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

- R01 只冻结所有权；不宣称 v1 已实现。
- R07 必须用真实 archive reader/writer 验证限额、preview token、revision/running guard、原子 commit 和回滚。
- 格式升级必须显式版本化并 fail-closed 拒绝 unknown field；不得用 `PRAGMA user_version` 代替 application wire version。

## Open items

- v1 wire、限额、preview、commit、平台和 legacy 迁移仍待实现。Owner: R07a、R07b、R07c、R07d、R07e。
