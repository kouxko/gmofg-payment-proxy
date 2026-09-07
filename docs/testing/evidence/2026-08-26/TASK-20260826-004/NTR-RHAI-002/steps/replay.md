# 真实链路复测步骤

1. 在 Proxy UI 导入并启用
   `examples/protocol-packages/nuvei_tango_rhai/dist/nuvei-tango-json-rhai-1.0.0.zip`，确认包 ID 与版本为
   `nuvei-tango-json-rhai@1.0.0`。
2. 保存当前 Workspace、Listener、端口和包启用状态快照；只在已确认的 Nuvei Socket Listener 绑定该包，
   其余 relay、上游 `tangodev.nuvei.com:9081` 和 TLS 设置保持不变。
3. 启动 Listener，由授权测试 App 依次产生三组测试交易。
4. 对同一连接/Exchange 保存安全阶段日志：方向、Frame 输入字节数、Decode 六字段、Display 结果、
   Encode 输出字节数、连接关联和结果。不得保存 JSON、Base64、PAN、Track2、MAC 或密钥。
5. PASS 判定：依次得到 1602/647、1602/914、1322/896 B 三组双向 Exchange；每个方向的 Frame、
   Decode、Display、Encode 全部成功；Encode 字节数等于输入；无包编译、Frame、Decode、Display、
   Encode、连接或 Listener failure。
6. 恢复测试前 Workspace、Listener、端口和包启用状态，并保存清理对照结果。

当前会话没有 Proxy 写入控制面或授权测试 App，本步骤未执行。
