# CURRENT-DOCUMENTS-001：当前模块与MCP文档一致性

任务TASK-20260905-001；结果PASS。时间、环境、HEAD、源文件SHA和完整命令见metadata.json。

核对Application Cargo路径依赖仅domain/product-api，模块图改为package-runtime/package-contract；保存命令为rule_definition_save；当前工具目录和说明均为34只读+5环境工具。
资源标题测试与实际工具目录测试均1/1 PASS；architecture.log记录架构文档、crate/前端边界与任务所有权扫描PASS。

复测从仓库根目录执行：

```sh
deno task scan:architecture
cargo test --manifest-path src-tauri/Cargo.toml --lib mcp::tests::resources_include_authoring_manifest_and_official_component
cargo test --manifest-path src-tauri/Cargo.toml --lib mcp::tests::tool_catalog_preserves_the_existing_read_only_runtime_and_portable_protocol_tools
```

resources目录保留当次实际检查的文档、Cargo和MCP权威源内容，来源路径与SHA见metadata。current-authority.log记录静态核对结果；无代码迁移或历史档案改写。根crate调试测试链接器报告既有大型__eh_frame警告，测试成功，没有把警告隐藏为无警告构建。

测试输入为源码与活动文档；预期为依赖/命令/工具清单一致，实际见各log。网络报文、TLS、截图、设备和外部服务N/A；未触发CI或发布。

最终补充：同步Android严格路由、heartbeat临时forward/业务gate边界、generation原子清理、bootstrap游标和按事件刷新、Listener局部冲突解决。新增三个resources快照对应最终活动文档；最终架构检查见[architecture-final.log](../integrated-verification/architecture-final.log)。#3恢复行为未改变，相关恢复文档待产品细节确认后同步。
