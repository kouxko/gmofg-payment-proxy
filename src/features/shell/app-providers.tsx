import { AppRuntime } from "@/features/shell/app-runtime";

export function AppProviders({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  // Route children are intentionally replaced by the persistent desktop workspace.
  void children;
  return <AppRuntime />;
}
