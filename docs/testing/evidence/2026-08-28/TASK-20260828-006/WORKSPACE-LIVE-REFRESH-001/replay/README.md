# 复测步骤

1. 使用隔离应用目录启动最新正式 App，并保持 Workspace 管理页打开。
2. 记录当前 Workspace 列表与详情的 Listener 数量和 revision。
3. 通过 MCP Environment candidate 对当前 Workspace 执行一次可确认的配置变更。
4. 不切换页面，等待 `snapshot_required`。
5. 核对顶部、列表和详情使用同一 Listener 数量与 revision。
6. 在名称输入框保留未保存文本后重复步骤 3，确认名称保留，但 Listener 数量和 revision 更新。
7. 模拟详情读取失败，确认页面明确标记刷新前快照；首次读取失败时显示详情暂不可用，不显示错误的“选择一个 Workspace”。

成功判定：页面无需轮询或切页即可读取权威新快照，旧请求不能覆盖，读取失败不把旧数据冒充为最新状态。
