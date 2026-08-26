# 复测步骤

从仓库根目录执行：

```bash
cargo fmt \
  --manifest-path examples/protocol-packages/nuvei_tango_rhai/tests/Cargo.toml \
  -- --check
cargo clippy \
  --manifest-path examples/protocol-packages/nuvei_tango_rhai/tests/Cargo.toml \
  --all-targets -- -D warnings
cargo test \
  --manifest-path examples/protocol-packages/nuvei_tango_rhai/tests/Cargo.toml
python3 -m compileall -q \
  examples/protocol-packages/nuvei_tango_rhai/build_package.py \
  examples/protocol-packages/nuvei_tango_rhai/tests/python_oracle.py
python3 examples/protocol-packages/nuvei_tango_rhai/build_package.py
shasum -a 256 \
  examples/protocol-packages/nuvei_tango_rhai/dist/nuvei-tango-json-rhai-1.0.0.zip
unzip -l \
  examples/protocol-packages/nuvei_tango_rhai/dist/nuvei-tango-json-rhai-1.0.0.zip
```

PASS 判定：6 个测试全部通过；Clippy 零警告；相同源码连续构建 SHA-256 相同；ZIP 只有
`display.rhai`、`document.toml`、`manifest.toml`、`protocol.rhai` 四个根文件；最终 SHA-256 为
`0595af171e20ae9eee21da42a8327971c99689a278cab6ffd7612ba20a4049ea`。
