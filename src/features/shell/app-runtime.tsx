"use client";

import { I18nProvider, Toast } from "@heroui/react";
import { AppShell } from "@/features/shell/app-shell";
import { WorkspaceContent } from "@/features/shell/workspace-content";
import { WorkspaceNavigationProvider } from "@/features/shell/workspace-navigation";

export function AppRuntime() {
  return (
    <I18nProvider locale="zh-CN">
      <Toast.Provider placement="top end" />
      <WorkspaceNavigationProvider>
        <AppShell>
          <WorkspaceContent />
        </AppShell>
      </WorkspaceNavigationProvider>
    </I18nProvider>
  );
}
