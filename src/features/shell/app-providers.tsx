"use client";

import dynamic from "next/dynamic";

const AppRuntime = dynamic(
  () =>
    import("@/features/shell/app-runtime").then((module) => module.AppRuntime),
  { ssr: false },
);

export function AppProviders({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return <AppRuntime>{children}</AppRuntime>;
}
