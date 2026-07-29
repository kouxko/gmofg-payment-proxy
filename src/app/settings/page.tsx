"use client";

import dynamic from "next/dynamic";

const SettingsView = dynamic(
  () =>
    import("@/features/settings/settings-view").then(
      (module) => module.SettingsView,
    ),
  { ssr: false },
);

export default function SettingsPage() {
  return <SettingsView />;
}
