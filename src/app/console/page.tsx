import { ConsoleRoute } from "@/features/console/console-route";

/** Next.js 静态路由入口；数据加载和错误状态由 ConsoleRoute 统一处理。 */

export default function ConsolePage() {
  return <ConsoleRoute />;
}
