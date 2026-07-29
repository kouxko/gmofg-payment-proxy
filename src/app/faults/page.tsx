"use client";

import dynamic from "next/dynamic";

const FaultsView = dynamic(
  () =>
    import("@/features/faults/faults-view").then(
      (module) => module.FaultsView,
    ),
  { ssr: false },
);

export default function FaultsPage() {
  return <FaultsView />;
}
