"use client";

import dynamic from "next/dynamic";

const BreakpointsView = dynamic(
  () =>
    import("@/features/breakpoints/breakpoints-view").then(
      (module) => module.BreakpointsView,
    ),
  { ssr: false },
);

export default function BreakpointsPage() {
  return <BreakpointsView />;
}
