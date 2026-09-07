import { CaptureRoute } from "@/features/capture/capture-route";

/** Next.js 静态路由入口；启动快照会由 CaptureRoute 交给实时抓包页面复用。 */

export default function CapturePage() {
  return <CaptureRoute />;
}
