"use client";

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
} from "react";

export type WorkspacePath =
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
