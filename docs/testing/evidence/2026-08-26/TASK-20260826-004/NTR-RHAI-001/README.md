# NTR-RHAI-001：Nuvei Tango Rhai 包级 parity 与 ZIP 验证

- 任务：`TASK-20260826-004`
- 派生自：`TASK-20260826-003 / NUVEI-PKG-003`
- 执行时间：2026-08-26 16:18:08 至 16:33:09 +08:00
- 结果：PASS

## 目的

使用仓库当前 Rhai Host runtime 直接编译和执行最终 ZIP，并把完全相同的合成 Frame 同时交给
`TASK-20260826-003` Python codec，验证 Frame、六字段结构与类型、只读 Encode、Display 原文展示、
fail-closed 行为、已知 Exchange 字节数和确定性构建。

## 被测对象与输入

- 源码：`examples/protocol-packages/nuvei_tango_rhai/`。
- 最终 ZIP：`resources/nuvei-tango-json-rhai-1.0.0.zip`，SHA-256
  `0595af171e20ae9eee21da42a8327971c99689a278cab6ffd7612ba20a4049ea`。
- 合成输入：`resources/request.json`、`resources/response.json`，来源分别为包内同名 fixture。
- Python oracle：`examples/external-packages/nuvei_tango_json/nuvei_tango_json/codec.py`。
- Rhai runtime：`intercept-proxy-protocol-scripting` 当前工作区源码。

所有输入都是合成非支付数据。没有使用或保存真实 PAN、Track2、MAC、密钥或完整交易报文。

## 验证结果

- 6/6 Rust 包级测试 PASS；ZIP 通过安全读取、Manifest/Schema/Rhai 编译并实际执行。
- 同一 upstream request 和 downstream response 的 Frame decision、body length、控制头、sequence、
  message type、字段类型及 byte-exact Encode 与 Python oracle 一致。
- Rhai `json_preview` 与 Display 保留全部合成字段名和值，不包含 `[redacted]`；HTML 特殊字符被转义。
- 六字段逐一修改和删除、context 篡改、跨方向复用全部在 Encode 失败，未产生返回字节。
- TCP 3 B 分段、缺 1 B、粘包、最小/最大长度、非法 sequence、非法 JSON、数组/空对象顶层均按合同处理。
- 1602/647、1602/914、1322/896 B 六个合成 Frame 在对应方向全部完成 Decode/Display/Encode，
  Encode 输出与输入逐字节相同。
- `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、Python `compileall` 和
  `git diff --check` PASS。
- 连续两次构建输出相同 SHA-256；ZIP 固定包含 4 个根文件，时间戳均为 1980-01-01 00:00。

具体期望、实际和比较见 `outputs/python-expected.json`、`outputs/rhai-actual.json` 与
`outputs/comparison.json`。命令输出摘要见 `outputs/tests-and-static.txt`、`outputs/zip-listing.txt` 和
`outputs/checksum.txt`。

## N/A

- 非法 UTF-8：N/A，任务明确排除。
- 重复 JSON key：N/A，任务明确排除。
- 真实交易输入：N/A，本用例仅验证合成包级行为；真实 Listener/交易另见 `NTR-RHAI-002`。
- CI：N/A，未获授权触发远程 CI。
