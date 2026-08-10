# 开发与扩展指南

## 1. 修改功能前先确定边界

新增需求时先写清楚：

- 用户操作是什么；
- Rust Use Case 是什么；
- 领域不变量是什么；
- 需要哪个 infrastructure/runtime 能力；
- 返回哪个 ViewModel 或事件；
- 哪些自动化测试可以证明它。

不要从页面按钮直接开始写网络逻辑。

## 2. 常见改动应该放哪里

| 改动 | 首选位置 |
| --- | --- |
| 新增字段及合法性规则 | `domain` |
| 新增保存、启动、删除等用户用例 | `application/facade` |
| 新增 SQLite、证书、ADB、文件适配 | `infrastructure` |
| 新增 HTTP/TLS/连接行为 | `proxy` |
| 新增包级弱网行为 | `android-engine` |
| 新增 Tauri 命令 | `src-tauri/src/commands`，只做薄适配 |
| 新增页面控件 | `src/features`，只提交用户意图 |

## 3. 注释原则

注释重点解释“为什么”，尤其是：

- 跨资源操作的顺序和补偿；
- 锁、generation、revision 和 runtime epoch 的所有权；
- TLS/mTLS 四类证书材料的区别；
- 超时从哪个时刻开始、覆盖哪个阶段；
- fail-open/fail-closed 的产品选择；
- 看似可以简化但会破坏协议语义的分支。

简单赋值、显而易见的 if 和类型名不需要逐行复述。函数较复杂时，在函数头写步骤总览，
在关键状态转换处写原因。

## 4. 文件大小与模块拆分

CI 对手写 Rust、TypeScript/TSX、Kotlin/Java 执行 500 行硬门禁。达到上限前应按职责拆分：

- 数据模型与执行逻辑分开；
- 配置解析与网络运行分开；
- 页面列表、详情、编辑器和 hooks 分开；
- 测试按行为主题拆分子模块；
- `mod.rs`/入口文件只保留公开接口和装配。

不要为了行数把一个状态机机械切成互相跳转的碎片。拆分后每个文件应能用一句话说明职责。

## 5. 错误模型

新增错误时必须提供稳定错误码和中文说明。可恢复错误补充 suggested action；字段错误绑定到
Rust DTO 字段；网络运行错误携带 entity ID/runtime epoch。前端只展示，不解析 message。

## 6. 测试顺序

1. 为领域不变量写单元测试。
2. 为 adapter/runtime 写失败路径和资源清理测试。
3. 为 IPC 写参数、错误和事件顺序测试。
4. 前端只测试 ViewModel 渲染和用户意图。
5. 运行 `pnpm check`。
6. Android 改动额外运行 Release lint、单元测试和 assembleRelease。
7. 涉及真实设备网络时单独记录真机证据，不把自动化结果冒充真机结果。

## 7. 提交前检查

```bash
pnpm scan:source-size
pnpm check
git diff --check
```

Android Companion 相关改动还应运行：

```bash
cd android-companion
gradle --no-daemon :app:testDebugUnitTest :app:lintRelease :app:assembleRelease
```

## 8. 评审清单

- 领域规则是否只实现了一次？
- 前端是否开始推断业务状态？
- 运行任务是否持有不可变快照？
- 失败发生在中间步骤时是否有补偿？
- 锁是否跨越可能阻塞的网络/JNI 调用？
- 密码、私钥、P12 是否可能进入 DTO 或日志，或绕过可移植导出的白名单与明确确认？
- 取消、超时和应用关闭是否释放端口、TUN、任务和容量？
- 新文件是否职责单一且小于门禁？
- 自动化证明与真机证明是否清楚区分？
