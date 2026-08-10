import type { WorkspacePath } from "@/features/shell/workspace-navigation";
import type { PageHelpGuide } from "./page-help-content";

type DiagnosticHelpPath = Extract<WorkspacePath, "/diagnostics">;

export const diagnosticPageHelpGuides: Record<
  DiagnosticHelpPath,
  PageHelpGuide
> = {
  "/diagnostics": {
    title: "诊断日志",
    summary:
      "按时间展示 Rust 已产生的 ADB、设备网络、代理入口、TLS 与 HTTP 脱敏诊断事件。没有记录不等于该阶段成功。",
    recommendedFor:
      "设备无网络、代理未收到请求、TLS 握手失败或停止网络接管异常时，用于判断失败发生在哪一层。",
    sections: [
      ["区分两条 ADB 通道", "adb forward 只承载桌面控制命令，adb reverse 才承载设备到桌面入口的业务连接。"],
      ["检查设备网络接管", "查看已产生的 Companion、VPN、TUN、目标应用和透明路由事件；缺失阶段需要结合设备状态继续确认。"],
      ["检查代理链路", "查看已产生的桌面 DNS、代理入口、客户端 TLS、上游 TLS 和 HTTP 会话事件。"],
      ["检查停止与清理", "查看已有的停止、回退和映射清理事件；不要把缺少成功日志视为已完成。"],
      ["筛选与保密边界", "可按设备、入口、方案或摘要筛选；Rust 会限制长度并脱敏密码、PEM 和长 Base64 内容。"],
    ].map(([title, detail], index) => ({
      id: `诊断日志-${index}`,
      title,
      steps: [
        "先按时间从最早失败记录向后检查，不要只看最后一条错误。",
        detail,
        "成功、警告和失败由 Rust 统一标记，前端不推断业务结果。",
        "修正配置后重新执行操作，并用实际产生的后续事件验证链路是否继续推进。",
      ],
    })),
  },
};
