"use client";

/** 把启动快照中已有的最近抓包页交给 CaptureView，减少首次进入时的空白等待。 */

import { CaptureView } from "@/features/capture/capture-view";
import { useBootstrap } from "@/features/shell/bootstrap-context";

export function CaptureRoute() {
  const { bootstrap } = useBootstrap();
  return <CaptureView initialPage={bootstrap?.recent_capture} />;
}
