"use client";

import dynamic from "next/dynamic";

const SessionsView = dynamic(
  () =>
    import("@/features/sessions/sessions-view").then(
      (module) => module.SessionsView,
    ),
  { ssr: false },
);

export default function SessionsPage() {
  return <SessionsView />;
}
