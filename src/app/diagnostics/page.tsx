import { DiagnosticLogsView } from "@/features/diagnostics/diagnostic-logs-view";

/** Next.js 静态路由入口；诊断数据由客户端视图通过 Rust IPC 查询。 */

export default function DiagnosticsPage() {
  return <DiagnosticLogsView />;
}
