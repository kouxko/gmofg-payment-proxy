import { SettingsView } from "@/features/settings/settings-view";

/** Next.js 静态路由入口；设置草稿的校验、保存和生效判断全部在 Rust。 */

export default function SettingsPage() {
  return <SettingsView />;
}
