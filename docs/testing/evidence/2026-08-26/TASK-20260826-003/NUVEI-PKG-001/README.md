# NUVEI-PKG-001：Nuvei Tango 只读 Python 外部协议包

- 任务：`TASK-20260826-003`
- 执行时间：2026-08-26 14:25:34 至 14:40:14 +08:00
- 结果：PASS

## 目的

证明新增 Python 外部包能够严格拆分 4 字节大端长度前缀的 Tango JSON Socket 报文，提供掩码后的
只读 Document，并且只在 Document 未变化时逐字节返回原始 frame。

## 环境

- macOS；Python 3.14.7
- OpenSSL 3.6.3
- websockets 17.0.1
- 被测源码：`examples/external-packages/nuvei_tango_json/`

## 输入与预期

- 自动化输入：`inputs/synthetic-contract.json` 描述的合成、非支付帧；实际生成逻辑位于
  `examples/external-packages/nuvei_tango_json/tests/test_codec.py`。
- 预期：分段返回 `need_more`；粘包只消费首帧；decode/encode 字节一致；敏感键掩码；任何字段、
  context 或方向变化均拒绝；WebSocket API 1 注册成功。

## 实际结果

- TDD RED：包尚不存在时，2 个测试模块因 `ModuleNotFoundError` 失败。
- GREEN：12 个单元/本机 WebSocket 测试全部 PASS。
- `compileall` PASS。
- wheel 构建 PASS，产物名 `nuvei_tango_json-1.0.0-py3-none-any.whl`。
- 额外只读观察：从用户报告在内存提取一条完整后台响应，647 字节；声明 body 643 字节；消息类型
  `AccptrCmpltnAdvcRspn`；Proxy 收到与发回 App 的字节一致；package decode/encode 字节一致；7 个
  敏感字段被掩码。

## 敏感资源说明

用户报告包含真实支付字段，未复制到证据、源码、fixture 或输出。实际报告检查只输出非敏感元数据和
布尔结果，且不联网重放。该观察不是可公开重放 fixture；正式自动化验收使用合成输入。

## 复测

执行 `steps/replay.md` 中的命令。真实支付报文重放：N/A，避免重复交易且不属于只读包验收范围。

## 不适用项

- Rust、Tauri、前端、数据库、构建和 App UI：N/A，本任务未修改这些边界。
- 真实后台发送：N/A，本任务禁止重放授权报文。
- CI：N/A，未获授权触发外部 CI。
- 对抗审查：N/A，用户明确这是低优先级小任务且无须执行。
