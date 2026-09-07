# 固定测试 Root CA 回归

- 执行时间：`2026-09-02 18:27:59 +08:00` 至 `2026-09-02 18:29:44 +08:00`
- 目的：确认 Proxy 恢复固定测试 Root，且公开证书与 Android Payment 内置 `server.crt` 完全一致。
- 结果：`PASS`

## 命令与结果

1. `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`：PASS。
2. `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-product-api`：6 passed，0 failed。
3. `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure certificates_tests:: -- --nocapture`：39 passed，0 failed。
4. `deno run -A --unstable-detect-cjs node_modules/vitest/vitest.mjs run src/features/certificates/certificates-view.test.tsx`：1 file、8 tests passed。
5. `cmp -s src-tauri/resources/certificates/intercept-proxy-test-root-ca.crt /Users/codin/Code/jp_gmofg_payment/app/src/main/assets/server.crt`：退出码 0，文件逐字节一致。
6. `openssl x509 -in src-tauri/resources/certificates/intercept-proxy-test-root-ca.crt -noout -fingerprint -sha256 -subject -issuer`：指纹为 `B4:72:77:A5:8D:81:AD:EB:3C:CE:59:7A:15:58:85:4D:AB:3D:0B:30:AB:CE:15:06:5A:FB:73:33:9B:CB:D7:4C`，Subject 与 Issuer 均为 `CN=Intercept Proxy TEST ONLY Root CA`。
7. `git diff --check`：PASS。

## 覆盖与限制

- 两个独立 Proxy 存储共享完全相同的固定 Root 和私钥，叶子证书及 SAN 仍按监听 IP 独立签发。
- 启动同步固定 Root 幂等；遇到非当前 Root 时保持 fail-closed，且不修改现有材料。
- 固定 Root 能冻结为运行时签发材料并验证动态叶子证书。
- A920MAX 到 `10.0.28.99` 的真实 TLS 握手未执行：本次没有部署远端新构建，也没有清理或迁移远端持久化证书。
- 对抗审查：按用户 `2026-09-02` 最新明确要求跳过。
- 远程 CI：`NOT_RUN`，未授权触发。
