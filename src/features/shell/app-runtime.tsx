"use client";

import { I18nProvider, RouterProvider, Toast } from "@heroui/react";
import { useCallback } from "react";
import { useRouter } from "next/navigation";
import { AppShell } from "@/features/shell/app-shell";

export function AppRuntime({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  const router = useRouter();
  const navigate = useCallback(
    (href: string) => {
      router.push(href);
    },
    [router],
  );

  return (
    <RouterProvider navigate={navigate}>
      <I18nProvider locale="zh-CN">
        <Toast.Provider placement="top end" />
        <AppShell>{children}</AppShell>
      </I18nProvider>
    </RouterProvider>
  );
}
