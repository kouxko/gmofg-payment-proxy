"use client";

import { CaptureView } from "@/features/capture/capture-view";
import { useBootstrap } from "@/features/shell/bootstrap-context";

export function CaptureRoute() {
  const { bootstrap } = useBootstrap();
  return <CaptureView initialPage={bootstrap?.recent_capture} />;
}
