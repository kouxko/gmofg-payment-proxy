# Nuvei Tango JSON Read-Only WebAssembly Component

`component/` 是 `nuvei-tango-json-rhai@1.0.1` 的唯一仓库实现。包 ID 为兼容既有安装身份而保留
`rhai` 名称，但当前实现是 Rust WebAssembly Component，不包含 Rhai 或 ZIP 运行路径。

包只观察 Nuvei Tango 的长度前缀 JSON Frame。Decode 保留完整 JSON preview，Display 将 object 和
array 递归渲染为嵌套 HTML table；Encode 仅允许 Document 完全不变时返回原始 Frame，任何字段修改、
删除、上下文篡改或跨方向复用均 fail-closed。

构建和局部回归：

```bash
cargo test --manifest-path examples/protocol-packages/nuvei_tango_rhai/component/Cargo.toml --locked
pnpm build:protocol-packages
```

可导入产物：

```text
dist/protocol-package-components/intercept-proxy-nuvei-tango-json-rhai-component.wasm
```

直接执行 `cargo build --target wasm32-wasip2` 只生成未追加顶层 Manifest 的原始产物；导入时必须使用
统一构建入口生成的文件。合成 JSON 回归向量保存在 `component/tests/fixtures/`。
