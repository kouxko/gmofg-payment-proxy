# CERTIFICATE-REVISION-SEMANTICS-001

- 任务：`TASK-20260904-004`
- 目的：证明证书集合 revision 推进后，未变化的固定 Root/Leaf 记录仍可用于元数据状态查询，同时保留逐记录材料一致性校验。
- 环境：macOS，仓库根目录 `/Users/codin/Code/gmofg-payment-proxy`，Rust workspace `src-tauri/Cargo.toml`。
- 被测状态：Git HEAD `cd7ba24604031b109e4e1093f65deeb178508fff` 加本任务未提交变更；同一工作区存在不修改证书 Rust 文件的 TASK-20260904-003 前端变更。

## 前置条件与输入

回归测试在内存 SQLite 中先写入 revision 1 的 `local_root_ca` 与 `proxy_leaf`，再写入 revision 2 的环境证书记录，使集合 revision 为 2 而固定证书记录 revision 保持 1。

文件型资源：`N/A`；本用例使用代码内确定性记录，不依赖外部证书、网络、设备或临时文件。

## 步骤与结果

1. RED：仅加入回归测试并执行：

   `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure certificate_status_accepts_unchanged_records_from_older_collection_revision -- --nocapture`

   结果：`FAIL`，稳定得到 `CERTIFICATE_INVALID / 证书元数据修订号与聚合修订号不一致。`

2. GREEN：移除状态查询中错误的“记录 revision 必须等于集合 revision”约束，保留元数据中 revision 必填，并再次执行同一命令。

   结果：`PASS`，1 passed，状态返回集合 revision 2、Root/Leaf 就绪。

3. 完整受影响 crate 回归：

   `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure`

   结果：`PASS`，467 个 lib tests、7 + 24 + 7 + 8 个 integration tests 全部通过，共 513 个测试通过，0 failed。

4. 静态检查：

   `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`

   `cargo clippy --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --all-targets -- -D warnings`

   `git diff --check`

   结果：全部 `PASS`。

## 预期与实际

- 预期：集合 revision 表示集合 CAS 代次；旧记录无需被伪造为新版本；记录解密时仍要求元数据 revision 与同一受保护材料 revision 一致。
- 实际：状态查询接受集合 revision 2 + 固定记录 revision 1；完整证书、SQLite、环境配置和 runtime 回归均通过。
- 字节/字段比较：`N/A`；未改变固定证书 DER、私钥或 TLS 报文。
- UI、网络、真实设备：`N/A`；本次是本地持久化语义修复，用户未要求安装或真实链路验收。

## 复测入口

从仓库根目录执行上述 focused test 与完整 infrastructure crate test；无需额外环境变量或外部服务。
