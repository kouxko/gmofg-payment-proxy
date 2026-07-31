"use client";

/**
 * WebView 的前端根运行环境。
 *
 * I18nProvider 统一 HeroUI 日期、日历和无障碍文案为中文；Toast.Provider 提供
 * 全局操作反馈；WorkspaceNavigationProvider + AppShell 共同组成不会随页面切换
 * 重建的桌面工作区。这里不创建任何业务数据源，数据源始终是 Rust。
 */

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
