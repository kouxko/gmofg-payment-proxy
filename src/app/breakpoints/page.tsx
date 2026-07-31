import { BreakpointsView } from "@/features/breakpoints/breakpoints-view";

/** Next.js 静态路由入口；真正暂停的网络任务始终由 Rust 断点协调器持有。 */

export default function BreakpointsPage() {
  return <BreakpointsView />;
}
