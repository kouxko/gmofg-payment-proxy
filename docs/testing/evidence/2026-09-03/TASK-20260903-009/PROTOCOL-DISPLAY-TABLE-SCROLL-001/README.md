# PROTOCOL-DISPLAY-TABLE-SCROLL-001

- 目的：验证协议 Display 宽表使用独立横向滚动容器，且规则删除改动没有破坏相关规则页面。
- 被测版本：`c2fd81e9382a7adc36615ec7d64a03f267fc4605` 加当前工作区任务修改。
- 执行时间：2026-09-03 16:45:10 至 16:46:43 +08:00。
- 环境：macOS，Deno/Vitest，Next.js 16.2.12，Rust/Cargo。

## 步骤与结果

1. 执行定向前端测试，4 个文件、35 个测试全部通过。
2. 执行 `deno task typecheck`、`deno task lint`、`cargo fmt --check` 与 `git diff --check`，全部通过。
3. 执行 `deno task build`，Next.js 生产构建成功并生成 13 个静态页面。
4. 执行规则运行时回归：同一连接先命中 Mock，再将运行时快照中的规则停用，下一条消息返回空动作列表；1 个测试通过。

完整命令与摘要见 `outputs/test-summary.txt`。

## 不适用项

- 原始报文、TCP chunks、外部 Server 响应：N/A，本用例是安全 Display 布局合同与规则运行时状态转换测试，不发送业务请求。
- UI 截图与人工横向滚动验证：N/A，用户明确表示自行验证 UI；本次以清洗输出 DOM/CSS 合同测试和正式构建作为自动化证据。

## 复测

从仓库根目录执行 `outputs/test-summary.txt` 中列出的命令。
