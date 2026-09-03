# AU EFTEX WebAssembly Component

`component/` 是 `au-eftex@1.1.0` 的唯一仓库实现。它以 Rust 编写并构建为单文件
WebAssembly Component，由 Proxy 在同一进程内加载，不连接 `/packages` WebSocket，也不使用
Python、JSON-RPC 或 Base64 transport。

Component 实现 Socket WIT 的 Frame、Decode、Encode 和 Display。DUKPT、3DES-OFB、H01、长度头、
ISO 8583 字段投影、敏感字段展示和 MAC fail-closed 合同保持不变。当前产物仅用于历史测试记录回放，
内置公开 ANSI DUKPT 测试 BDK `0123456789ABCDEFFEDCBA9876543210`，不得用于生产交易或真实 BDK。

构建和局部回归：

```bash
cargo test --manifest-path examples/external-packages/au_eftex/component/Cargo.toml --locked
pnpm build:protocol-packages
```

可导入产物：

```text
dist/protocol-package-components/intercept-proxy-au-eftex-component.wasm
```

直接执行 `cargo build --target wasm32-wasip2` 只生成未追加顶层 Manifest 的原始产物；导入时必须使用
统一构建入口生成的文件。

## 线路与安全合同

- 39 字节明文 H01 头；H01 前可带 2 字节大端 Socket 长度头。
- 密文区从 4 字节 ASCII MTI 开始，采用传统 2-key 3DES DUKPT 和 3DES-OFB。
- Decode 将已声明 ISO 8583 域投影为字段级 Document；未声明字段明确失败。
- Track2、PIN、DE53、DE64 和私有二进制域只显示长度或掩码。
- 含 DE64/DE128 的报文只允许逐字节不变的 observe-only 往返；字段变化返回
  `MAC_REPLACEMENT_REQUIRED`。
- Component 源码与测试只使用公开测试 BDK 和合成非支付数据。
