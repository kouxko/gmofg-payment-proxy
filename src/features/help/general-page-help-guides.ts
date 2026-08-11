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
    "代理入口只决定客户端连接本机的地址、端口和请求去向。TLS/mTLS 与证书均按入口独立配置；Android 客户端或上游 Server 未要求 mTLS 时，客户端身份可以留空。入口本身不添加模拟。",
    ["理解入口与模拟的区别", "新增或复制入口", "配置普通 TLS 或 mTLS", "测试握手、保存并启动", "添加故障模拟或规则"],
  ),
  "/android-network": generalGuide(
    "应用网络接管",
    "只接管设备网络方案中明确选择的安卓应用，由 Rust 完成透明代理路由、弱网执行、配置校验和状态判断。",
    ["检查连接工具", "选择目标设备", "安装设备端组件", "配置设备网络方案", "启动与恢复"],
  ),
};
