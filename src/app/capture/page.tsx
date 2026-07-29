"use client";

import dynamic from "next/dynamic";

const CaptureRoute = dynamic(
  () =>
    import("@/features/capture/capture-route").then(
      (module) => module.CaptureRoute,
    ),
  { ssr: false },
);

export default function CapturePage() {
  return <CaptureRoute />;
}
