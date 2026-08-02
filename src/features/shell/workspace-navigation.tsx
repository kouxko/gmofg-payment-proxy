"use client";

/**
 * 桌面工作区的“轻量导航器”。
 *
 * Tauri 加载的是 Next.js 静态文件。若每次点击左侧导航都走浏览器级路由，整个
 * WebView 会重建，顶部状态栏、事件订阅和页面草稿就会闪烁或丢失。因此这里仅
 * 在 React 内保存 pathname/searchParams，由 WorkspaceContent 切换页面内容。
 * 这也是“点击 Tab/导航时外壳不能整页刷新”的核心实现。
 */

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
} from "react";

export type WorkspacePath =
  | "/workspaces"
  | "/listeners"
  | "/android-network"
  | "/console"
  | "/capture"
  | "/sessions"
  | "/breakpoints"
  | "/rules"
  | "/faults"
  | "/certificates"
  | "/settings";

type WorkspaceLocation = {
  pathname: WorkspacePath;
  searchParams: URLSearchParams;
};

type WorkspaceNavigation = WorkspaceLocation & {
  navigate: (href: string) => void;
};

const defaultPath: WorkspacePath = "/console";
const workspacePaths = new Set<WorkspacePath>([
  "/workspaces",
  "/listeners",
  "/android-network",
  "/console",
  "/capture",
  "/sessions",
  "/breakpoints",
  "/rules",
  "/faults",
  "/certificates",
  "/settings",
]);

function parseWorkspaceLocation(href: string): WorkspaceLocation {
  // 使用虚拟 origin 只为复用 URL 解析能力；这里不会发起任何网络请求。
  const url = new URL(href, "http://workspace.local");
  const pathname = workspacePaths.has(url.pathname as WorkspacePath)
    ? (url.pathname as WorkspacePath)
    : defaultPath;
  return {
    pathname,
    searchParams: new URLSearchParams(url.search),
  };
}

const WorkspaceNavigationContext =
  createContext<WorkspaceNavigation | null>(null);

export function WorkspaceNavigationProvider({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  const [location, setLocation] = useState<WorkspaceLocation>(() => {
    if (typeof window === "undefined") {
      return parseWorkspaceLocation(defaultPath);
    }
    return parseWorkspaceLocation(
      `${window.location.pathname}${window.location.search}`,
    );
  });
  const navigate = useCallback((href: string) => {
    // 不调用 location.href 或 Next Router，保证 AppShell 和 Rust 事件订阅常驻。
    setLocation(parseWorkspaceLocation(href));
  }, []);
  const value = useMemo(
    () => ({ ...location, navigate }),
    [location, navigate],
  );

  return (
    <WorkspaceNavigationContext.Provider value={value}>
      {children}
    </WorkspaceNavigationContext.Provider>
  );
}

export function useWorkspaceNavigation() {
  const value = useContext(WorkspaceNavigationContext);
  if (!value) {
    throw new Error(
      "useWorkspaceNavigation 必须在 WorkspaceNavigationProvider 内使用",
    );
  }
  return value;
}
