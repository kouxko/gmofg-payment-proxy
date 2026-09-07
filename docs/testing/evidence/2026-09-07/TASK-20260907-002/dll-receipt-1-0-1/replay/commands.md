# 复测入口

在仓库根目录执行。需要 Rust/Cargo、已安装 wasm32-wasip2 target、Deno；版本见 inputs/environment.json。
包为独立 Cargo workspace，包命令必须指向 examples 下 Cargo.toml；Host 命令使用 src-tauri/Cargo.toml。
当前回归使用 examples/protocol-packages/gmofg_payment_dll/tests/fixtures；历史输入快照在 ../resources/package/tests/fixtures。
测试源码快照在 ../resources/package/src 与 ../resources/gmofg_payment_dll_component.rs。
无需启动应用、连接设备、配置证书或访问真实 GMO-FG 服务。

```bash
cargo test --locked --all-targets --manifest-path examples/protocol-packages/gmofg_payment_dll/Cargo.toml
cargo fmt --manifest-path examples/protocol-packages/gmofg_payment_dll/Cargo.toml --check
cargo clippy --locked --all-targets --manifest-path examples/protocol-packages/gmofg_payment_dll/Cargo.toml -- -D warnings
deno check examples/protocol-packages/gmofg_payment_dll/build.mjs
deno run -A examples/protocol-packages/gmofg_payment_dll/build.mjs
cargo test --locked --manifest-path src-tauri/Cargo.toml -p intercept-proxy-package-runtime --test gmofg_payment_dll_component -- --nocapture
cargo clippy --locked --manifest-path src-tauri/Cargo.toml -p intercept-proxy-package-runtime --test gmofg_payment_dll_component -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure strict_http_package_codec_reads_original_utf8_and_shift_jis_wire_bytes --lib -- --nocapture
rustfmt --edition 2024 --check src-tauri/crates/package-runtime/tests/gmofg_payment_dll_component.rs
git diff --check
```

成功判定：包18 tests、Host 1 test、codec1 test通过；无Clippy警告；构建输出精确版本1.0.1；
Host日志 RECEIPT_EVIDENCE 每条含实际输入、Document、编码输出和HTML；四条未修改输入逐字符相同，
四条修改结果ErrorCode=D48且ErrorMessage保留日文及特殊字符，HTML特殊字符正确转义。
输出名 dist/gmofg-payment-dll-1.0.1.wasm，导入后选 gmofg-payment-dll@1.0.1。
旧1.0.0保留，勿覆盖或以旧文件名分发新版。复测输出另存新目录，不覆盖本历史证据。

清理：本次未创建Listener/服务/设备状态，无运行状态需恢复；构建仅写可再生产物和Cargo缓存。
