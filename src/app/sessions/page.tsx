import { SessionsView } from "@/features/sessions/sessions-view";

/** Next.js 静态路由入口；会话筛选、分页和详情数据全部通过 Rust IPC 获取。 */

export default function SessionsPage() {
  return <SessionsView />;
}
