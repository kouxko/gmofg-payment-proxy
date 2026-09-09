测试正文是按截图场景构造的日文 Shift_JIS 向量，不冒充截图原报文。活动 fixture：`src-tauri/src/runtime_logs/exchange_ui_layer/tests/fixtures/receipt-shift-jis.bin`。版本 1。

Tauri build-script 的 APK 前置条件复用既有归档，无需重复保存 24 MB 同字节文件：
- 父任务 TASK-20260904-006，用例 manual-trigger-and-artifact。
- 仓库路径 `docs/testing/evidence/2026-09-07/TASK-20260904-006/manual-trigger-and-artifact/resources/artifact/resources/android-companion.apk`。
- SHA-256 `78f37319c586013f821d9d349cbd7e96092b9b707b2839a1f24fc1f2514b3dc8`，与本次使用的 `src-tauri/resources/android-companion.apk` 完全一致。
- 仅复制到上述 build-script 路径；未启动或验收 Android。
