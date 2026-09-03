# STANDALONE-WEAK-NETWORK-UI-001

## 目的

验证 Android 弱网配置页已收敛为普通用户可直接使用的独立弱网流程，并保留折叠后的代理、范围、
运行保护和专家参数；同时验证四个有来源的快捷场景只写入现有 `WeakNetworkProfile` 字段。

## 环境与被测状态

- 执行时间：2026-09-03 15:46:18 +08:00
- 平台：macOS arm64
- 分支：`codex/intercept-proxy-generalization`
- 基线提交：`d2346ca167e884927873922bb785ec07398b3e97`
- Deno：`2.9.6`
- 被测对象：当前工作区中的 Android 弱网 React 组件、帮助内容和用户操作指南
- 测试期间状态：定向自动化完成后未再修改被测文件；仓库同时存在其他任务的未提交修改，均未纳入本用例

## 输入与预期

| 场景 | 公开输入 | 项目字段预期 |
| --- | --- | --- |
| 参考 2G | 上行 256 Kbps、下行 280 Kbps、RTT 400 ms | 上行 32000 B/s、下行 35000 B/s、单向固定延迟 200 ms、丢包 0 |
| 参考慢速 3G | 上下行 400 Kbps、RTT 200 ms | 上下行 50000 B/s、单向固定延迟 100 ms、丢包 0 |
| 参考慢速 4G | 上行 750 Kbps、下行 1.6 Mbps、RTT 150 ms | 上行 93750 B/s、下行 200000 B/s、单向固定延迟 75 ms、丢包 0 |
| 完全断网 | 项目既有完全丢包语义 | 随机丢包 10000 基点，其余常用限制清零或不限制 |

换算合同：公开带宽按十进制 `Kbps / 8 = B/s`；公开 RTT 除以 2 后写入项目的逐方向固定延迟。
来源：sitespeed.io throttle presets 与 Google Lighthouse throttling 文档。

## 步骤、命令与实际结果

1. 先增加默认折叠与场景映射测试；实现前分别观察到 1 个和 3 个预期失败，证明测试能够捕获缺失行为。
2. 执行 Android 网络和帮助内容定向回归：

   ```bash
   deno run -A --unstable-detect-cjs node_modules/vitest/vitest.mjs run src/features/android-network src/features/help/page-help.test.tsx --reporter=dot
   ```

   实际：最终复跑 9 个测试文件、90 项测试全部通过，耗时 21.32 秒。

3. 执行类型检查：

   ```bash
   deno task typecheck
   ```

   实际：PASS。

4. 对本任务修改的 TypeScript/TSX 文件执行 ESLint。

   实际：PASS。

5. 执行生产前端构建：

   ```bash
   deno run -A --unstable-detect-cjs node_modules/next/dist/bin/next build
   ```

   实际：PASS；编译与 TypeScript 检查通过，生成 13 个静态页面，包含 `/android-network`。

6. 执行源码大小门禁：

   ```bash
   deno task scan:source-size
   ```

   实际：FAIL。仅报告 6 个本任务未修改的既有超长文件；本任务调整后的
   `src/features/android-network/android-network-view.test.tsx` 为 499 行，未进入违规列表。

7. 执行补丁空白检查：

   ```bash
   git diff --check
   ```

   实际：PASS。

8. 用户检查时指出设备控制卡底部三个按钮没有填满三等分横向空间；按钮改为填满各自网格列，
   并增加布局 class 回归。独立审查随后发现并关闭两项问题：同一 Profile 的权威参数返回后场景高亮同步，
   以及“完全断网 → 风险确认 → 启动参数 true”的集成覆盖。最终复审结论：APPROVE。

## 验收结果

- PASS：弱网无需代理路由即可配置并启动；默认流程明确显示该合同。
- PASS：首屏只保留常用效果和场景入口；代理、范围、运行保护和专家项默认折叠，标题保留状态摘要。
- PASS：常用丢包百分比无损转换为现有整数基点字段。
- PASS：四个场景的带宽、延迟和丢包映射均有精确自动化断言。
- PASS：选择“自定义”不改写当前参数；手工修改常用字段后回到自定义状态。
- PASS：原有运行 owner、安全确认、保存和启动命令合同保持不变。
- PASS：安装、更新、授权三个设备操作按钮在宽屏三等分列内等宽填满，窄屏仍转为单列。
- PASS：独立代码审查最终 APPROVE，P0/P1/P2 均为 0。
- NOT_RUN：界面观感、点击式手工验收和截图，由用户自行检查，本用例未生成占位截图。
- NOT_RUN：Android 真机弱网数据面；本任务不修改 Rust/Android Engine，且用户本轮只要求界面简化与场景入口。
- NOT_RUN：CI、提交、推送和发布，未获得相关授权。
- 已知非阻塞项：仓库级源码大小门禁仍被 6 个既有、非本任务文件阻断。

## 复测入口

依次执行本页第 2 至第 7 步；手工验收时进入 Android 弱网页面，选择设备和应用后检查首屏场景、
展开“更多设置”，并分别保存/启动无代理路由的自定义方案与四个快捷场景。

## 不适用项

- 二进制报文、TCP Frame、Decode/Encode、Server 响应：N/A，本任务只改变前端配置呈现和现有字段映射。
- 测试资源快照：N/A，输入均为代码内常量及本页记录的公开数值，没有外部文件型资源。
- expected/actual 独立 JSON：N/A，精确字段比较由 Vitest 断言保存于活动测试源码。
