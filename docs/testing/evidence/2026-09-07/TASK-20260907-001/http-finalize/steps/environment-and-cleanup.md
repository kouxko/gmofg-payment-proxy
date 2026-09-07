# 准备与清理

1. 使用 inputs/environment.json 记录的仓库版本与 resources/source 的实测内容；当前回归直接运行仓库活动测试。历史复现应在隔离 checkout 中对照 source-changes.patch 和 SHA256 恢复本任务内容，不覆盖共享工作区。
2. 安装 Rust/Cargo 1.98.0、wasm32-wasip2 target 和 Deno 2.9.6；依赖按归档 Cargo.lock 与仓库 Deno lock 获取。Wasm fixture 使用 wit-bindgen 0.48.1，Host 使用 Wasmtime 48.0.1。前端工具按仓库正常依赖安装步骤准备。
3. 在仓库根目录执行 `python3 <本证据目录>/replay/run.py <仓库根目录>`。它在新的系统临时目录保存每条命令日志与新的 wire，不覆盖本档案。
4. wire 用例在 loopback 随机空闲端口创建专用单客户端 Listener 和专用单连接 origin；使用唯一 X-Path-Id 关联两个 TCP 段。固定上游目标、HTTP 明文、Protocol 包和规则配置由归档 real_wasm_finalize_wire.rs 与 production_shape.rs 精确定义，测试过程无需 UI、认证、证书或用户持久配置。
5. schema100 用例创建独立 tempfile 数据库，保存合法 Workspace revision=7、选中 Workspace 和旧包（disabled），关闭后重开并逐字段/逐字节比较，再加载旧包 Decode/Encode。资源原件为 resources/packages/json-pretty-1.0.0.wasm；源测试路径中读取的是相同 SHA256 的活动示例。
6. wire 已停止 Listener、deactivate runtime 并等待 origin 任务结束；connection.json 中 listener_cleaned_up=true，测试还检查监听端口关闭。数据库由 tempfile 作用域删除。没有修改当前 App Workspace、安装包或启动常驻进程。保留 Cargo 构建缓存和本证据文件以供复测。

TLS/mTLS、DNS、真实设备、业务系统、UI、CI、制品发布均 N/A 或 NOT_RUN；本测试只声明本地实际 TCP/HTTP 完整交换。
