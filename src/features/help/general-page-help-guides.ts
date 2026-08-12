import type { WorkspacePath } from "@/features/shell/workspace-navigation";
import type { PageHelpGuide } from "./page-help-types";

type GeneralHelpPath = Extract<
  WorkspacePath,
  "/workspaces" | "/listeners" | "/android-network"
>;

function generalGuide(
  title: string,
  summary: string,
  sections: readonly [string, string, string, string, string],
): PageHelpGuide {
  return {
    title,
    summary,
    recommendedFor: `需要在${title}页面完成配置、核对 Rust 返回结果并保留明确操作边界时使用。`,
    sections: sections.map((section, index) => ({
      id: `${title}-${index}`,
      title: section,
      steps: [
        `先读取页面显示的当前状态，再开始${section}。`,
        "所有列表、校验和持久化结果均以 Rust 命令返回值为准。",
        "输入只在当前表单中收集，不在浏览器侧实现代理业务规则。",
        "出现错误时保留 Rust 返回的字段和消息，修正后重新执行操作。",
      ],
    })),
  };
}

export const generalPageHelpGuides: Record<GeneralHelpPath, PageHelpGuide> = {
  "/workspaces": generalGuide(
    "Workspace 管理",
    "集中管理通用代理 Workspace，并通过 Rust 完成创建、选择、复制、导入、导出、校验、保存和删除。",
    ["选择 Workspace", "新建与复制", "编辑与保存", "导入与导出", "删除与恢复"],
  ),
  "/listeners": generalGuide(
    "代理入口配置",
    "每个入口明确选择 HTTP 或 Socket 数据平面。HTTP 保留请求解析、固定 Server 与 MITM；Socket 按原始字节转发，并可选择 Transparent 或 TLS Bridge。证书与启停仍按入口独立配置。",
    ["选择 HTTP 或 Socket", "配置目标与安全模式", "按 TLS 方向绑定证书", "测试连接、保存并启动", "查看连接与双向字节"],
  ),
  "/android-network": generalGuide(
    "应用网络接管",
    "只接管设备网络方案中明确选择的安卓应用，由 Rust 完成透明代理路由、弱网执行、配置校验和状态判断。",
    ["检查连接工具", "选择目标设备", "安装设备端组件", "配置设备网络方案", "启动与恢复"],
  ),
};
