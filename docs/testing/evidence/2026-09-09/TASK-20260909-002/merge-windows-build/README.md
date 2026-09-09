# Body 字符集修复交付与 Windows 构建

任务 TASK-20260909-002；当前执行中。先合并最新 develop，再创建/合并 MR，最后 API 手动构建 android-windows-build。完整测试/发布流程不执行。

本地集成检查：抓包 31/31、tsc、前端边界 PASS，命令与输出见 outputs/integration-checks.json 及对应日志。

resources 保存实际 CI 与版本配置；inputs 保存精确请求，outputs 保存远程响应和制品校验；replay 提供只读复验。

N/A：本任务不涉及新协议向量、真实交易、Windows 安装/运行和设备验收。
