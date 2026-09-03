# Nuvei Tango JSON WebAssembly Component

`component/` 是 `nuvei-tango-json@1.0.1` 的唯一仓库实现。它是 Rust WebAssembly Component，
由 Proxy 在同一进程内加载，不连接 `/packages` WebSocket，也不使用 Python 或 JSON-RPC。

包只读地拆分并解析 Nuvei Tango 的长度前缀 JSON 报文。Decode 输出经过掩码的 JSON preview，
Display 将 object 和 array 递归渲染为嵌套 HTML table；Encode 仅在 Document 未变化且原始输入与
认证上下文匹配时返回原始 Frame。

构建和局部回归：

```bash
cargo test --manifest-path examples/external-packages/nuvei_tango_json/component/Cargo.toml --locked
pnpm build:protocol-packages
```

可导入产物：

```text
dist/protocol-package-components/intercept-proxy-nuvei-tango-json-component.wasm
```

直接执行 `cargo build --target wasm32-wasip2` 只生成未追加顶层 Manifest 的原始产物；导入时必须使用
统一构建入口生成的文件。
