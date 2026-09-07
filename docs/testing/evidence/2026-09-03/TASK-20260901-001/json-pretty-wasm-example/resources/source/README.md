# JSON Pretty Wasm Component

`json-pretty@1.0.0` 是一个 HTTP 协议包示例。它把请求和响应 Body 解析为递归 JSON Document，
在 Display 阶段输出经过 HTML 转义的缩进 JSON，并使用安全内联样式为 key、string、number、
boolean 和 null 提供编辑器式语法分色；规则修改 Document 后输出缩进 JSON。

未修改 Document 时，Encode 原样返回最初的 HTTP Body，保留原有空白和字段顺序。非法 JSON 在
Decode/Encode 边界 fail-closed，不会以空对象或原文替代失败。

Manifest 的上下行均不声明 Schema；Display 能格式化 object、array 和 JSON 标量。

当前 Proxy 按 HTTP 合同允许上下行 Schema 为 `null`，并将 HTTP 方向能力投影为
`frame: false`。Display 输出的内联视觉 CSS 会经过属性和值白名单过滤；脚本、事件、外链资源、
`<style>`、定位和尺寸覆盖仍会被删除。

## 构建与测试

从仓库根目录执行：

```bash
deno run -A examples/protocol-packages/json_pretty/build.mjs
```

包内构建器会运行原生 Rust 测试，使用 `wasm32-wasip2` 生成 Component，把
`manifest.json` 写入 `intercept-proxy:manifest` 自定义 section，并输出：

```text
examples/protocol-packages/json_pretty/dist/json-pretty-1.0.0.wasm
```

同目录的 `json-pretty-1.0.0.wasm.sha256` 保存产物校验值。

只运行包级测试：

```bash
cargo test --locked --all-targets \
  --manifest-path examples/protocol-packages/json_pretty/Cargo.toml
```

本示例保持自包含，不加入仓库统一 Component 清单。
