import { FaultsView } from "@/features/faults/faults-view";

/** Next.js 静态路由入口；故障模板最终由 Rust 转换成普通拦截规则。 */

export default function FaultsPage() {
  return <FaultsView />;
}
