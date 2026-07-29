"use client";

import dynamic from "next/dynamic";

const ConsoleRoute = dynamic(
  () =>
    import("@/features/console/console-route").then(
      (module) => module.ConsoleRoute,
    ),
  { ssr: false },
);

export default function ConsolePage() {
  return <ConsoleRoute />;
}
