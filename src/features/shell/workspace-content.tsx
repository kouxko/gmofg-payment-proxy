"use client";

/**
 * 持久桌面外壳中的页面选择器。
 *
 * 所有页面组件都运行在同一个 AppShell/BootstrapProvider 下面；这里只替换中央
 * 内容，而不是重新加载 HTML。这能保留顶部状态、Tauri Channel 和当前页面外的
 * 全局状态。页面自己的临时草稿仍由各自组件管理。
 */

import { BreakpointsView } from "@/features/breakpoints/breakpoints-view";
import { CaptureRoute } from "@/features/capture/capture-route";
import { CertificatesView } from "@/features/certificates/certificates-view";
import { ConsoleRoute } from "@/features/console/console-route";
import { FaultsView } from "@/features/faults/faults-view";
import { RulesView } from "@/features/rules/rules-view";
import { SessionsView } from "@/features/sessions/sessions-view";
import { SettingsView } from "@/features/settings/settings-view";
import { AndroidNetworkView } from "@/features/android-network/android-network-view";
import { ListenersView } from "@/features/listeners/listeners-view";
import { ProtocolPackagesView } from "@/features/protocol-packages/protocol-packages-view";
import { WorkspacesView } from "@/features/workspaces/workspaces-view";
import { DiagnosticLogsView } from "@/features/diagnostics/diagnostic-logs-view";
import { useWorkspaceNavigation } from "./workspace-navigation";

export function WorkspaceContent() {
  const { pathname } = useWorkspaceNavigation();

  switch (pathname) {
    case "/workspaces":
      return <WorkspacesView />;
    case "/listeners":
      return <ListenersView />;
    case "/protocol-packages":
      return <ProtocolPackagesView />;
    case "/android-network":
      return <AndroidNetworkView />;
    case "/diagnostics":
      return <DiagnosticLogsView />;
    case "/capture":
      return <CaptureRoute />;
    case "/sessions":
      return <SessionsView />;
    case "/breakpoints":
      return <BreakpointsView />;
    case "/rules":
      return <RulesView />;
    case "/faults":
      return <FaultsView />;
    case "/certificates":
      return <CertificatesView />;
    case "/settings":
      return <SettingsView />;
    case "/console":
    default:
      return <ConsoleRoute />;
  }
}
