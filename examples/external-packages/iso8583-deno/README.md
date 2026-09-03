# ISO 8583 ASCII WebAssembly Component

`component/` 是 `iso8583-deno-ascii@1.0.1` 的唯一仓库实现。包 ID 为兼容既有安装身份而保留
`deno` 名称，但当前实现是 Rust WebAssembly Component，不包含 Deno 或 TypeScript 运行路径。

它实现一个明确受限的 ISO 8583 Socket Profile：2 字节大端长度头、ASCII MTI、二进制位图、
ASCII LLVAR/LLLVAR，以及清单声明的字段集合。未声明字段、错误长度、非法字符集和无法重建的报文
均 fail-closed。

构建和局部回归：

```bash
cargo test --manifest-path examples/external-packages/iso8583-deno/component/Cargo.toml --locked
pnpm build:protocol-packages
```

可导入产物：

```text
dist/protocol-package-components/intercept-proxy-iso8583-deno-ascii-component.wasm
```

直接执行 `cargo build --target wasm32-wasip2` 只生成未追加顶层 Manifest 的原始产物；导入时必须使用
统一构建入口生成的文件。
