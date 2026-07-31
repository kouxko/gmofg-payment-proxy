# 通用代理核心与 Payment 产品层边界

## 目标

Rust 后端必须同时满足两个独立目标：

1. GMO-FG Payment 桌面产品保持现有 UI、IPC、规则、证书和真机行为。
2. HTTP/TLS 代理核心可以在不加载 Payment 资产、Shift-JIS 或固定双通道配置的情况下，被其他产品、未来 TUI/CLI 和无 UI 测试复用。

“能脱离 Tauri 运行”不等于“产品无关”。完成标准是所有 Payment 决策只存在于 `product-payment`。

## 目标依赖图

```text
Tauri / future TUI / future CLI
                |
                v
              host  <---------------- product-payment
                |                         |
       +--------+---------+               |
       v        v         v               |
 application  runtime  infrastructure     |
       |                    |              |
       +--------> domain <--+--------------+
                ^
                |
           product-api
```

约束：

- `product-api` 只定义扩展契约。
- `domain`、`application`、`runtime`、`infrastructure`、`host` 是通用 core。
- `product-payment` 可以依赖 core 和 `product-api`。
- `product-payment` 的默认库依赖仅允许产品契约、编解码和静态资产；需要
  runtime、infrastructure、Tokio、PKCS12 的真机诊断入口必须放在显式
  `real-device-tool` feature 后，不能污染其他产品复用的默认依赖图。
- core 不得直接或间接依赖 `product-payment`。
- Tauri 只选择并注入产品 Profile，不实现产品规则。

## 通用 core 所有权

- 原始 HTTP/1.1 请求、响应、Header、状态码和 Body 字节。
- TLS accept/connect、mTLS 可选能力、超时、取消和连接生命周期。
- 任意 `ChannelId` 的 Listener 与上游配置。
- 会话、报文版本、断点状态机、规则评估框架和命中计数。
- Header/状态码动作、延迟、抖动、限速、间歇网络、截断和断连等网络 primitive。
- 容量、分页、筛选、事件游标、SQLite、原子文件导出和平台密钥保护。
- 证书生成、解析、PKCS12 解析和 X.509 校验 primitive；资产由产品层提供。

## 可复用组合与内部职责

所有展示层都从 `ApplicationHostBuilder` 进入。桌面 Tauri、未来 TUI/CLI
或无 UI 测试只需要提供：

1. 一个实现 `ProductProfile` 的产品配置；
2. 文件选择和密钥保护等 `HostPlatformServices`；
3. 可选的 `ProxySupervisorPort` 测试替身。

Host 使用具名 `ApplicationDependencies` 注入代理、抓包、会话、断点、规则、
故障、证书、设置、导出和事件端口。展示层只持有 `Application`，不直接获得
数据库、TLS 私钥或 runtime 对象。

`Application` 的公开命令契约保持为一个稳定 facade，内部按用例拆分：

- `facade/traffic.rs`：抓包、会话和断点；
- `facade/rules.rs`：规则编辑、校验、持久化和故障模板；
- `facade/settings.rs`：设置校验、保存、重启和回滚事务；
- `facade/validation.rs`：跨展示层一致的规范化值和字段错误；
- `facade.rs`：应用启动/关闭、代理生命周期、证书和共享发布守卫。

Runtime 侧的 `RuntimePipelineAdapter` 只保留 transport trait 适配和工作流编排。
规则快照、命中计数、CAS 提交和冲突重试由内部 `RuleRuntimeService` 串行协调；
HTTP/1 原始 Head 捕获和字节保持 I/O 位于 `transport/raw_http1.rs`。这些内部
模块不改变公开 API，目的是让每个高风险状态机可以独立审查和测试。

## `product-payment` 所有权

- `transaction` 与 `dll` 通道目录及中文显示名。
- 默认端口 `16627` 与 `16127`。
- GMO-FG 默认上游、Payment App/GMO-FG Server 文案。
- Shift-JIS 严格编解码及 JSON Body 解释。
- Payment 请求 ID、请求类型、D48 业务观察字段解析。
- Payment 故障模板及其默认参数。
- 统一测试 Root CA、测试签名私钥、内置 Payment `server.crt`。
- Payment 兼容 DTO 映射与真机 DLL 测试入口。

真机入口属于产品验证工具，不属于产品 Profile 的库契约。构建方式：

```bash
cargo build \
  -p gmofg-proxy-product-payment \
  --features real-device-tool \
  --bin real-device-dll-proxy
```

## 强制架构测试

- core crate 源码不得包含产品术语、固定端口或产品资产引用。
- core 的 Cargo 依赖树不得包含 `product-payment`。
- `product-payment` 默认依赖树不得包含 runtime、infrastructure 或真机探针依赖。
- Runtime 使用 `alpha`、`beta`、`gamma` 三个任意通道完成启动和停止。
- Host 使用不含 Payment 资产的 Test Profile 完成构建、查询和关闭。
- Payment Profile 保持现有两个通道、端口、Shift-JIS 严格失败、Root CA 导出和默认上游 CA。
- Tauri commands 只能依赖 Application/Host 契约。
- Payment 无 UI 黄金测试和真机矩阵与通用 core 测试分别报告。

## 迁移顺序

1. 建立 `product-api`、`product-payment` 和依赖方向守卫。
2. 将固定 Channel enum 替换为 `ChannelId` 与数据驱动目录。
3. 将 Shift-JIS/JSON 解释迁出 domain/application/runtime。
4. 将证书资产与产品证书文案迁入 `product-payment`。
5. 将故障模板、请求分类和产品文案迁入 `product-payment`。
6. 拆分 Runtime Pipeline 的通用桥接与产品解释职责；规则动作转换和原始报文
   投影分别放入无状态内部模块，协调器只编排连接、会话、断点和事件生命周期。
7. 使用 Test Profile 证明 core 可独立运行。
8. 使用 Payment Profile 黄金测试和真机矩阵证明兼容性。

任一步都不得以通过窄测试代替完整行为证据。
