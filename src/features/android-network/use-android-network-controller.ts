"use client";

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { toast } from "@heroui/react";
import type { AndroidNetworkProfile } from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import {
  androidNetworkControlOverview,
  callCommand,
  errorMessage,
} from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import {
  mergeAndroidDeviceTargets,
  runtimeOwnerQueryKey,
  runtimeResponseMatches,
} from "./android-runtime-owner-model";

export function useAndroidNetworkController() {
  const adb = useIpcQuery("android-adb", () => callCommand(commands.androidAdbGet()));
  const devices = useIpcQuery("android-devices", () => callCommand(commands.androidDeviceList()));
  const profiles = useIpcQuery("android-profiles", () => callCommand(commands.deviceNetworkProfileList()), []);
  const overview = useIpcQuery("android-control-overview", androidNetworkControlOverview);
  const owners = overview.data?.owners ?? [];
  const targets = mergeAndroidDeviceTargets(devices.data ?? [], owners);
  const [selectedSerialOverride, setSelectedSerialOverride] = useState<string>();
  const selectedSerial = selectedSerialOverride ?? adb.data?.selected_serial ?? undefined;
  const selectedOwner = owners.find((owner) => owner.serial === selectedSerial);
  const runtimeTargetKey = runtimeOwnerQueryKey(selectedOwner);
  const packages = useIpcQuery(
    `android-packages:${selectedSerial ?? "none"}`,
    () => callCommand(commands.androidPackageList(selectedSerial!)),
    [],
    { enabled: Boolean(selectedSerial) },
  );
  const [packageFilterDraft, setPackageFilterDraft] = useState("");
  const [packageFilter, setPackageFilter] = useState("");
  const filteredPackages = useIpcQuery(
    `android-packages:${selectedSerial ?? "none"}:filter:${packageFilter}`,
    () => callCommand(commands.androidPackageQuery(selectedSerial!, packageFilter)),
    [],
    { enabled: Boolean(selectedSerial && packageFilter) },
  );
  const [lastActiveProfileIds, setLastActiveProfileIds] = useState<Record<string, string | undefined>>({});
  const runtime = useIpcQuery(
    `android-runtime:${runtimeTargetKey ?? "none"}`,
    async () => {
      const owner = selectedOwner!;
      const response = await callCommand(commands.deviceNetworkStatus(owner.serial));
      if (runtimeResponseMatches(owner, response)) {
        setLastActiveProfileIds((current) => {
          if (response.state !== "stopped" && response.state !== "faulted" && response.active_profile_id) {
            return current[owner.serial] === response.active_profile_id
              ? current
              : { ...current, [owner.serial]: response.active_profile_id };
          }
          if (!(owner.serial in current)) return current;
          const next = { ...current };
          delete next[owner.serial];
          return next;
        });
      }
      return response;
    },
    undefined,
    { enabled: Boolean(runtimeTargetKey) },
  );
  const [drafts, setDrafts] = useState<Record<string, AndroidNetworkProfile | undefined>>({});
  const draft = selectedSerial ? drafts[selectedSerial] : undefined;
  const setDraft = useCallback((next: AndroidNetworkProfile | undefined) => {
    if (!selectedSerial) return;
    setDrafts((current) => ({ ...current, [selectedSerial]: next }));
  }, [selectedSerial]);
  const endpoints = useIpcQuery(
    `android-endpoints:${selectedSerial ?? "none"}:${draft?.id ?? "none"}:${selectedOwner?.epoch ?? "none"}`,
    () => callCommand(commands.deviceNetworkEndpoints(selectedSerial!, draft?.id ?? null)),
    undefined,
    { enabled: Boolean(selectedSerial) },
  );
  const displayedRuntime = runtime.data && selectedOwner
    && runtimeResponseMatches(selectedOwner, runtime.data)
    ? runtime.data
    : undefined;
  const activeProfileId = displayedRuntime?.active_profile_id
    ?? (displayedRuntime?.state === "running" && selectedSerial
      ? lastActiveProfileIds[selectedSerial]
      : undefined);

  const refreshDevices = devices.refresh;
  const refreshOverview = overview.refresh;
  const refreshRuntime = runtime.refresh;
  const refreshEndpoints = endpoints.refresh;
  const runtimeRefreshInFlight = useRef<Promise<void> | null>(null);
  const runtimeRefreshQueued = useRef(false);
  const refreshRuntimeSerially = useCallback((): Promise<void> => {
    runtimeRefreshQueued.current = true;
    if (runtimeRefreshInFlight.current) return runtimeRefreshInFlight.current;
    const task = (async () => {
      while (runtimeRefreshQueued.current) {
        runtimeRefreshQueued.current = false;
        await refreshRuntime();
      }
    })().finally(() => {
      if (runtimeRefreshInFlight.current === task) runtimeRefreshInFlight.current = null;
    });
    runtimeRefreshInFlight.current = task;
    return task;
  }, [refreshRuntime]);

  useAppEventRefresh(["android_vpn_status_changed"], async () => {
    await Promise.all([refreshDevices(), refreshOverview()]);
  });
  useAppEventRefresh(
    ["android_vpn_status_changed"],
    async () => { await Promise.all([refreshRuntimeSerially(), refreshEndpoints()]); },
    { paused: !selectedOwner, entityId: selectedOwner?.serial },
  );
  useEffect(() => {
    let disposed = false;
    let timer: number | undefined;
    const poll = async () => {
      await Promise.all([
        refreshDevices(),
        refreshOverview(),
        runtimeTargetKey ? refreshRuntimeSerially() : Promise.resolve(),
      ]);
      if (!disposed) timer = window.setTimeout(() => void poll(), 1_000);
    };
    timer = window.setTimeout(() => void poll(), 1_000);
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [refreshDevices, refreshOverview, refreshRuntimeSerially, runtimeTargetKey]);

  const [pendingTargets, setPendingTargets] = useState<Record<string, string | undefined>>({});
  const [dangerousConfirmed, setDangerousConfirmed] = useState(false);
  const selectedSerialRef = useRef<string | null | undefined>(selectedSerial);
  const runtimeOwnerKeyRef = useRef(runtimeTargetKey);
  useLayoutEffect(() => {
    selectedSerialRef.current = selectedSerial;
    runtimeOwnerKeyRef.current = runtimeTargetKey;
  }, [runtimeTargetKey, selectedSerial]);
  const run = useCallback(async (
    name: string,
    action: () => Promise<void>,
    serial = selectedSerial ?? "global",
  ) => {
    if (pendingTargets[serial]) return;
    setPendingTargets((current) => ({ ...current, [serial]: name }));
    try {
      await action();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPendingTargets((current) => {
        if (current[serial] !== name) return current;
        const next = { ...current };
        delete next[serial];
        return next;
      });
    }
  }, [pendingTargets, selectedSerial]);

  return {
    adb, devices, profiles, overview, owners, targets, selectedSerial, selectedOwner,
    runtimeTargetKey, packages, packageFilterDraft, packageFilter, filteredPackages,
    runtime, displayedRuntime, activeProfileId, draft, setDraft, endpoints,
    pendingTargets, dangerousConfirmed,
    selectedSerialRef, runtimeOwnerKeyRef, setSelectedSerialOverride,
    setPackageFilterDraft, setPackageFilter, setDangerousConfirmed, run,
  };
}
