"use client";

import { useEffect } from "react";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";

export function useAndroidOwnerRefresh(
  refreshDevices: () => Promise<void>,
  refreshOwners: () => Promise<void>,
): void {
  useAppEventRefresh(["android_vpn_status_changed"], refreshOwners);
  useEffect(() => {
    let disposed = false;
    let timer: number | undefined;
    const poll = async () => {
      await refreshDevices();
      await refreshOwners();
      if (!disposed) timer = window.setTimeout(() => void poll(), 1_000);
    };
    timer = window.setTimeout(() => void poll(), 1_000);
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [refreshDevices, refreshOwners]);
}
