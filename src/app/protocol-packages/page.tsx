import { ProtocolPackagesView } from "@/features/protocol-packages/protocol-packages-view";

/**
 * Next.js 静态路由入口。
 *
 * 桌面应用内点击左侧导航时仍由 WorkspaceNavigationProvider 原地切换，保留
 * AppShell 与事件订阅；这个入口只覆盖直接加载静态路径的场景。
 */
export default function ProtocolPackagesPage() {
  return <ProtocolPackagesView />;
}
