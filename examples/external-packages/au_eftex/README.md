# AU EFTEX External Package

该目录同时保留两种交付方式：

- `au_eftex/`：原有 Python + WebSocket JSON-RPC 实现，用于快速调试和历史 trace 回放。
- `component/`：等价的 Rust WebAssembly Component 单文件实现，用于 Proxy 进程内导入和运行。

WebAssembly Component 直接使用 Socket WIT 的字节参数，`encode` 同时收到原始线路报文，因此不需要
Python 外部 RPC 为跨调用状态设计的 AES-GCM `encoding_context`。DUKPT、3DES-OFB、H01、长度头、
ISO 8583 字段投影、敏感字段展示和 MAC fail-closed 规则保持不变。

构建和运行局部回归：

```bash
cargo test --manifest-path examples/external-packages/au_eftex/component/Cargo.toml --locked
pnpm build:protocol-packages
```

产物是：

```text
dist/protocol-package-components/intercept-proxy-au-eftex-component.wasm
```

`cargo build --target wasm32-wasip2` 只生成未追加顶层 Manifest 的编译器原始产物；导入时必须使用
统一构建入口生成的 `dist` 文件。

当前 Component 是历史测试记录重放专用产物，源码内置公开 ANSI DUKPT 测试 BDK
`0123456789ABCDEFFEDCBA9876543210`，运行时不读取 `AU_EFTEX_BDK_FILE` 或
`AU_EFTEX_BDK_HEX`。该 Component 不得用于生产交易或真实 BDK；Python 外部调试包继续使用下述
运行时密钥配置。

`au-eftex@1.1.0` 是 Intercept Proxy 的外部 Socket 软件包。它通过 Python、WebSocket JSON-RPC 和
PyCryptodome 处理传统 2-key 3DES DUKPT 保护的 AU EFTEX 报文。

本实现选择 Python，是因为当前 Deno 2.9.5 的 `node:crypto` 不提供 trace 所需的
`des-ede3-ofb`，Web Crypto 也不支持 3DES。支付密码原语不应由项目自行手写。

## 已验证的密码与线路合同

- 39 字节明文 H01 头：`T`、DF00、DF01、DF02、DF03（10 字节 KSN）和 `B` 编码标记。
- H01 前可带 2 字节大端 Socket 长度头；兼容“长度仅计算后续 body”和“长度包含长度头自身”两种
  线路约定。拆包时严格校验，重编码时保留原报文采用的约定并重新计算长度。
- 密文区从 4 字节 ASCII MTI 开始，不加密 H01 头。
- 传统 2-key 3DES DUKPT：请求使用 Data Request key，响应使用 Data Response key；Data key
  包含标准单向变换，16 字节逻辑密钥按 `K1-K2-K1` 扩成 24 字节 TDES key。
- 数据密码为 3DES-OFB。每帧 IV 由 H01 DF02 的 6 位 STAN 动态生成：STAN 左侧补零为 16 位十进制、
  按 BCD 编码为 8 字节，再与 `01 23 45 67 89 AB CD EF` 逐字节异或；不是部署时固定配置。
- 对齐规则为 `FF * fill_count + 单字节 fill_count`，即使原文已经 8 字节对齐也增加完整填充块。
- ISO8583:1993 Profile 来自项目提供的 PAX `edc8583.xml`，支持二进制 primary/secondary bitmap，
  不支持 tertiary bitmap。字段覆盖 DE2/3/4/7/11/12/14/22/23/24/25/28/29/35/37/38/39/40/41/42/46/
  48/49/50/52/53/54/55/56/63/64/74/75/76/77/81/86/87/88/89/90/97/109/110/123/124/128；未声明字段会
  关闭当前业务调用，不会猜测长度或透明转发。

用户提供的交易 trace 仅在本机内存中用于验算，没有复制到源码、测试、文档或日志。请求与响应均已验证为：
派生有效 Data key 一致、解密后的完整字节一致、重新加密后的线路字节一致、拆包与粘包边界一致。

## 安装与测试

要求 Python 3.11 或更高版本：

```bash
cd examples/external-packages/au_eftex
python3 -m venv .venv
.venv/bin/python -m pip install --upgrade pip
.venv/bin/python -m pip install -e .
.venv/bin/python -m unittest discover -s tests -v
```

Windows 使用 `.venv\\Scripts\\python.exe`。

### 回放外部历史 trace

仓库根目录的验证器可以直接读取外部交易 trace，不会复制或改写原文件：

```bash
examples/external-packages/au_eftex/.venv/bin/python \
  scripts/verify_au_eftex_trace.py \
  "/path/to/Internal transaction trace.txt"
```

它会同时验证请求和响应方向的 IPEK、DUKPT transaction/Data key、3DES-OFB 解密、ISO8583
Document、Display hook、JSON-RPC hook 以及逐字节重加密结果。验证器还会复现 313 字节首段读取：
不完整报文必须返回 `need_more`，完整报文才返回 `complete`，避免把 TCP 分段误判为协议头错误。
验证器当前默认输出 MTI、字节数和布尔结果；如排障需要，可以扩展为保存完整 Document、报文和结果，并为输出
配置容量、轮转与保留期限。

## Python 外部调试包密钥配置

生产环境优先使用只有当前 OS 用户可读写的文件：

- `AU_EFTEX_BDK_FILE`：32 个十六进制字符，即 16 字节 BDK。

POSIX 系统会拒绝 group/other 具有任意权限的密钥文件。可以先用 `install -m 600 /dev/null <path>`
创建空文件，再通过部署环境的秘密管理工具写入；不要把真实值放进命令历史、仓库、Proxy 设置或日志。

仅用于隔离测试时，也可使用 `AU_EFTEX_BDK_HEX`。同一秘密的 `_FILE` 与 `_HEX` 不能同时配置；进程读取
`_HEX` 后会立即从自身环境映射删除。IV 按每帧 H01 STAN 计算，不接受固定 IV 配置。

其他配置：

- `EXTERNAL_PACKAGE_URL`：默认 `ws://127.0.0.1:8765/packages`，路径必须精确为 `/packages`。本示例自身默认
  接受 loopback `ws` 或任意 `wss`；这不是 Proxy 的包身份或来源门禁。
- `AU_EFTEX_ALLOW_INSECURE_REMOTE_WS=1`：启用本示例连接远端明文 `ws`。Proxy 对所有能到达服务且 wire 正确的
  外部包一律按受信任包处理，不要求 token、Origin、mTLS 或注册授权。
- `RECONNECT_DELAY_SECONDS`：默认 `1`。

启动：

```bash
.venv/bin/au-eftex
```

首次注册后，在 Proxy 的“协议包”页面启用 `au-eftex@1.1.0`，再把该精确版本绑定到 Socket listener。
软件包重连只恢复在线状态，不会自动启动已停止的 listener。

## Proxy 合同

- Proxy 主动且每连接仅调用一次 `package.register`；软件包不会主动注册。
- 上下行声明的方法为：
  - `hooks.<direction>.split_frame`
  - `hooks.<direction>.decrypt_message`
  - `hooks.<direction>.encrypt_message`
  - `document.<direction>.render_message`
- Decode 会把 bitmap 中已声明的 ISO8583 域投影为字段级 Document；DE4 金额使用 canonical `int`，
  DE48/52/55/64/128 使用 opaque blob，其他域使用 string 并保留业务前导零。DE55 的长度前缀是
  三位 ASCII，但 payload 保留原始二进制字节。
- `encoding_context` 是软件包使用进程内随机 AES-256-GCM key 加密并认证的 opaque blob，用于在独立
  encode RPC 中恢复 H01、KSN 和原始 ISO8583 报文；认证域同时绑定上下行方向。规则不应读取、修改或跨方向
  复用该字段，篡改和错误方向都会被拒绝。
- 无规则修改时，字段 `decode → encode`、DUKPT 重加密和原始线路字节均完全一致。
- Display 展示普通 ISO8583 字段；Track2、PIN、DE53、DE64 和私有二进制域只显示长度/掩码，不显示值。
- 单条 JSON-RPC wire message 最大 1 MiB；日志只记录连接、完整 hook 名、关联 token、字节数、字段名、
  耗时、结果分类和稳定错误码。

### 真机诊断日志

每个 RPC 会输出单行 JSON：

- `rpc_started`：关联 token、完整 hook 方法、方向、操作、输入字节数或 Document 字段数。
- `rpc_completed`：结果、耗时、frame `need_more/complete`、消费/输出字节数、已解析字段名或稳定错误码。
- 连接层：连接尝试、上线/断线、wire 超限、非法 JSON-RPC 和重连代次。

稳定错误码区分 `HEADER_INVALID`、`LENGTH_PREFIX_INVALID`、`KSN_INVALID`、`PADDING_INVALID`、
`DATA_KEY_DIRECTION_MISMATCH`、`DECRYPTED_MTI_INVALID`、
`ISO8583_PARSE_FAILED`、`ISO8583_ENCODE_FAILED`、`ENCODING_CONTEXT_INVALID`、
`MAC_REPLACEMENT_REQUIRED` 以及通用
frame/decode/encode/display 失败。当前实现默认输出元数据；允许按排障需要扩展为完整记录 Base64、Document、KSN、
ISO8583 域和报文，实际部署应设置容量、轮转和保留期限。

## 当前连接边界

Proxy 与外部软件包之间的 WebSocket 接口不提供应用层身份认证；所有能够到达配置地址且满足 wire 合同的进程均按
受信任外部包接纳。AU EFTEX 示例保留上述可配置的 `ws`/`wss` transport profile，但不把网络来源、Origin 或注册
身份当作 Proxy 授权条件。

这份 trace 能证明 DUKPT Data key、OFB、IV、填充、加解密范围和已观察字段布局，但没有给出可复现的
EFTEX MAC 算法。公开的 Retail MAC / ISO 9797-1 组合没有匹配该 trace 的 DE64，因此本版本不会伪造
“MAC 已验证”能力：

- DE64 作为 `message_authentication_code` blob 解析并原样保留；observe-only 往返不会改变 MAC。
- 不验证或重新生成 DE64。若原报文含 DE64，任何 ISO8583 字段变化（包括调用方同时替换 DE64）都会返回
  `MAC_REPLACEMENT_REQUIRED`，不会向线路发送未经验证的修改报文。当前只允许该类报文做字节不变的
  observe-only 往返。
- DE128 采用相同的 fail-closed 规则；任何带 DE128 的报文发生字段变化都会被拒绝。
- 在获得厂商 MAC 模式、padding、输入区间与输出截断合同前，不应把字段修改、LocalResponder 或业务验收视为可用。

BDK、PIN block、轨道数据和完整支付 trace 应视为敏感测试材料。Component 源码与自动化测试只使用上述
公开 ANSI DUKPT 测试 BDK 和合成非支付字节；真实 BDK 不得写入该源码或由该 Component 处理。
