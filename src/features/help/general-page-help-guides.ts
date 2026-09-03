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
    recommendedFor: `需要在${title}页面完成配置、核对操作结果并保留明确操作边界时使用。`,
    sections: sections.map((section, index) => ({
      id: `${title}-${index}`,
      title: section,
      steps: [
        `先读取页面显示的当前状态，再开始${section}。`,
        "所有列表、校验和保存结果均以页面显示为准。",
        "输入只在当前表单中收集，不在浏览器侧实现代理业务规则。",
        "出现错误时保留具体字段和消息，修正后重新执行操作。",
      ],
    })),
  };
}

export const generalPageHelpGuides: Record<GeneralHelpPath, PageHelpGuide> = {
  "/workspaces": generalGuide(
    "Workspace 管理",
    "集中管理通用代理 Workspace，包括创建、选择、复制、导入、导出、校验、保存和删除。",
    ["选择 Workspace", "新建与复制", "编辑与保存", "导入与导出", "删除与恢复"],
  ),
  "/listeners": generalGuide(
    "代理入口配置",
    "每个入口明确选择 HTTP 或 Socket 数据平面。HTTP 保留请求解析、固定 Server 与 MITM；Socket 按原始字节转发，并可选择 Transparent 或 TLS Bridge。证书与启停仍按入口独立配置。",
    ["选择 HTTP 或 Socket", "配置目标与安全模式", "按 TLS 方向绑定证书", "测试连接、保存并启动", "查看连接与双向字节"],
  ),
  "/android-network": {
    title: "应用网络接管",
    summary: "为选中的安卓应用单独运行弱网；代理调试、作用范围和专家参数均为按需设置。",
    recommendedFor: "需要快速验证安卓应用在延迟、抖动、丢包或限速环境下的表现时使用。",
    sections: [
      {
        id: "android-network-device",
        title: "选择设备",
        steps: [
          "连接 Android 设备并允许 ADB 调试。",
          "在目标设备中选择本次操作的设备。",
          "按页面状态安装或更新设备端组件。",
          "首次使用时按提示完成 VPN 授权。",
        ],
      },
      {
        id: "android-network-application",
        title: "选择目标应用",
        steps: [
          "新建或打开一个设备网络方案。",
          "在目标应用列表中选择需要测试的应用。",
          "可按包名筛选，点击整行即可选择或取消。",
          "弱网只接管方案中已选择的应用。",
        ],
      },
      {
        id: "android-network-common-effects",
        title: "设置常用弱网效果",
        steps: [
          "可先选择参考 2G、参考慢速 3G、参考慢速 4G 或完全断网场景。",
          "参考场景采用公开来源数值，并按页面说明把 RTT 换算为单向延迟。",
          "也可选择自定义，再填写延迟、延迟波动、丢包率和上下行限速。",
          "不配置代理入口也可以单独保存并启动弱网。",
        ],
      },
      {
        id: "android-network-more-settings",
        title: "按需展开更多设置",
        steps: [
          "只有需要改变默认运行保护时才展开运行保护。",
          "只有需要限制弱网地址时才展开限制弱网范围。",
          "只有需要把流量接入桌面代理时才展开同时接入代理调试。",
          "Burst、乱序、DNS 和 TCP/IP 故障参数位于专家参数。",
        ],
      },
      {
        id: "android-network-run",
        title: "保存、启动与恢复",
        steps: [
          "保存方案后由 Rust 统一校验字段和范围。",
          "点击启动后确认页面显示的设备和运行状态。",
          "修改运行中的方案后使用应用更新配置。",
          "测试完成后停止；异常时使用对应设备的紧急恢复。",
        ],
      },
    ],
  },
};
