"use client";

import { BreakpointsView } from "@/features/breakpoints/breakpoints-view";
import { CaptureRoute } from "@/features/capture/capture-route";
import { CertificatesView } from "@/features/certificates/certificates-view";
import { ConsoleRoute } from "@/features/console/console-route";
import { FaultsView } from "@/features/faults/faults-view";
import { RulesView } from "@/features/rules/rules-view";
import { SessionsView } from "@/features/sessions/sessions-view";
import { SettingsView } from "@/features/settings/settings-view";
import { useWorkspaceNavigation } from "./workspace-navigation";

export function WorkspaceContent() {
  const { pathname } = useWorkspaceNavigation();

  switch (pathname) {
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
