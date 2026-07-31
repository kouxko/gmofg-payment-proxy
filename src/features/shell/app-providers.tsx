import { AppRuntime } from "@/features/shell/app-runtime";

/**
 * Next.js RootLayout 与持久桌面工作区之间的适配层。
 *
 * Next.js 要求 layout 接收 route children，但桌面应用不能让 route children 随
 * 导航反复挂载，因此有意用 AppRuntime 统一接管显示。不要把业务逻辑加到这里。
 */

export function AppProviders({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  // 路由 children 被持久工作区有意替代；保留 void 可明确说明这不是遗漏。
  void children;
  return <AppRuntime />;
}
