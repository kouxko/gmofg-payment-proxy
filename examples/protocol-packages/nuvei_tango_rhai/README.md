# Nuvei Tango JSON Read-Only Rhai Package

`nuvei-tango-json-rhai@1.0.0` 是可直接导入 Intercept Proxy 的 Socket Rhai 协议包。它只观察
Nuvei Tango 的长度前缀 JSON Frame；任何 Document 字段变化都会在 Encode 阶段失败，包不会重建或
修改线路 JSON。

## 线路合同

- 4 字节大端 body length，长度不包含自身。
- 4 字节不透明控制头。
- 8 字节 ASCII 数字 sequence。
- 一个且仅一个顶层消息字段的 UTF-8 JSON object。
- body 最小 14 字节，完整 Frame 最大 1 MiB。

Document 固定包含 `frame_length`、`control_header`、`sequence`、`message_type`、
`json_preview` 和 `encoding_context` 六个字段。`json_preview` 与 Display 保留全部 JSON 字段名和
字段值，不脱敏、不掩码、不替换；不要把真实敏感交易报文用于截图、日志或测试 fixture。

Decode 只生成观察 Document。Encode 使用宿主提供的原始 `origin` 重新 Decode，并逐一比较六个字段；
完全一致时返回原始 Frame，任何修改、删除、context 篡改或跨方向复用都会 fail-closed，不产生线路
输出。包不推断 4 字节控制头含义，也不执行支付业务校验、MAC 校验或 JSON 重编码。

## 构建

从仓库根目录执行：

```bash
python3 examples/protocol-packages/nuvei_tango_rhai/build_package.py
```

产物：

- `dist/nuvei-tango-json-rhai-1.0.0.zip`
- `dist/nuvei-tango-json-rhai-1.0.0.zip.sha256`

构建器只使用 Python 标准库，以固定条目顺序、固定时间戳、Stored 模式和固定权限生成 ZIP；同一源码
连续构建的 ZIP 字节与 SHA-256 必须一致。

## 导入和启用

1. 打开 Intercept Proxy 的“协议包”页面，导入
   `dist/nuvei-tango-json-rhai-1.0.0.zip`。
2. 确认预览显示包 ID `nuvei-tango-json-rhai`、版本 `1.0.0`、Socket 上下行 Document 与四个阶段
   均可用，然后完成导入。
3. 启用 `nuvei-tango-json-rhai@1.0.0`。
4. 在 Nuvei Socket Listener 中选择该包；Listener 的 relay、上游目标和 TLS 设置沿用已确认配置，
   本包不修改连接或证书策略。
5. 启动 Listener 后分别检查 upstream/downstream 的 Frame、Decode、Display、Encode；任何阶段失败都
   应关闭当前连接并报告错误，禁止改为无包透传或默认成功。

## 复测

包级测试直接运行仓库现有 Rhai Host runtime，并把同一批合成 Frame 交给
`TASK-20260826-003` 的 Python codec 作为 oracle：

```bash
cargo test \
  --manifest-path examples/protocol-packages/nuvei_tango_rhai/tests/Cargo.toml
```

静态、构建和确定性检查：

```bash
cargo fmt \
  --manifest-path examples/protocol-packages/nuvei_tango_rhai/tests/Cargo.toml \
  -- --check
python3 -m compileall -q \
  examples/protocol-packages/nuvei_tango_rhai/build_package.py \
  examples/protocol-packages/nuvei_tango_rhai/tests/python_oracle.py
python3 examples/protocol-packages/nuvei_tango_rhai/build_package.py
shasum -a 256 \
  examples/protocol-packages/nuvei_tango_rhai/dist/nuvei-tango-json-rhai-1.0.0.zip
unzip -l \
  examples/protocol-packages/nuvei_tango_rhai/dist/nuvei-tango-json-rhai-1.0.0.zip
```

非法 UTF-8 和重复 JSON key 按任务合同为 N/A。包级测试包含 Python parity、TCP 分段与粘包、长度
边界、非法 sequence/JSON/顶层结构、六字段逐一修改与删除、context 篡改、跨方向复用、已知三组
双向字节数、ZIP 确定性和实际 ZIP 导入编译执行。
