"use client";

import { type ReactElement, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Alert, toast } from "@heroui/react";
import type {
  AndroidNetworkProfile,
  AndroidPackageViewModel,
  AndroidProfileEditIntent,
  AndroidRuntimeOwnerViewModel,
  WeakNetworkProfile,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { AdvancedNetworkCard } from "./advanced-network-card";
import { toneColor } from "@/lib/format";
import { DeviceControlCard } from "./device-control-card";
import {
  BasicNetworkParametersCard,
  DestinationTargetsCard,
} from "./network-parameter-cards";
import {
  ProfileActions,
  ProfileBasicsCard,
  ProfileSelectorCard,
  UnselectedProfileState,
} from "./profile-cards";
import { TargetApplicationsCard } from "./target-applications-card";
import { ProxyRoutesCard } from "./proxy-routes-card";
import { RuntimeEndpointsCard } from "./runtime-endpoints-card";
import { useCurrentWorkspaceListeners } from "./use-current-workspace-listeners";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import { runtimeAlertStatus } from "./android-network-types";
import {
  clearOwnerConditionally,
  mergeAndroidDeviceTargets,
  runtimeOwnerQueryKey,
  runtimeResponseMatches,
} from "./android-runtime-owner-model";
import { useAndroidOwnerRefresh } from "./use-android-owner-refresh";

/**
 * 页面只展示 Rust ViewModel、收集表单输入并发送用户意图。
 * 设备发现、规则校验、持久化和 VPN 状态判断均由 Rust 完成。
 */
export function AndroidNetworkView(): ReactElement {
  const adb = useIpcQuery("android-adb", () => callCommand(commands.androidAdbGet()));
  const devices = useIpcQuery("android-devices", () => callCommand(commands.androidDeviceList()));
  const profiles = useIpcQuery(
    "android-profiles",
    () => callCommand(commands.deviceNetworkProfileList()),
    [],
  );
  const workspaceListeners = useCurrentWorkspaceListeners();
  const runtimeOwners = useIpcQuery(
    "android-runtime-owners",
    () => callCommand(commands.deviceNetworkRuntimeOwners()),
  );
  const refreshRuntimeOwners = runtimeOwners.refresh;
  const refreshDevices = devices.refresh;
  const targets = mergeAndroidDeviceTargets(devices.data ?? [], runtimeOwners.data ?? []);
  const [selectedSerialOverride, setSelectedSerialOverride] = useState<string>();
  const selectedSerial = selectedSerialOverride ?? adb.data?.selected_serial ?? undefined;
  const selectedOwner = (runtimeOwners.data ?? []).find((owner) => owner.serial === selectedSerial);
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
  const runtime = useIpcQuery(
    `android-runtime:${runtimeTargetKey ?? "none"}`,
    () => callCommand(commands.deviceNetworkStatus(selectedOwner!.serial)),
    undefined,
    { enabled: Boolean(runtimeTargetKey) },
  );
  const runtimeData = runtime.data;
  const refreshRuntime = runtime.refresh;
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
      if (runtimeRefreshInFlight.current === task) {
        runtimeRefreshInFlight.current = null;
      }
    });
    runtimeRefreshInFlight.current = task;
    return task;
  }, [refreshRuntime]);
  useAndroidOwnerRefresh(refreshDevices, refreshRuntimeOwners);
  const refreshSelectedRuntimeEvent = useCallback(async (): Promise<void> => {
    await Promise.all([refreshRuntimeSerially(), refreshEndpoints()]);
  }, [refreshEndpoints, refreshRuntimeSerially]);
  useAppEventRefresh(
    ["android_vpn_status_changed"],
    refreshSelectedRuntimeEvent,
    {
      paused: !selectedOwner,
      entityId: selectedOwner?.serial,
    },
  );
  // 设备切换时查询结果会短暂保留上一台设备的数据。活动方案缓存必须按 serial 隔离，
  // 否则新设备会错误显示上一台设备正在运行的方案。
  const lastActiveProfileIds = useRef(new Map<string, string>());
  const displayedRuntime = runtimeData && selectedOwner && runtimeResponseMatches(selectedOwner, runtimeData)
    ? runtimeData
    : undefined;
  const activeProfileId = displayedRuntime?.active_profile_id
    ?? (displayedRuntime?.state === "running" && selectedSerial
      ? lastActiveProfileIds.current.get(selectedSerial)
      : undefined);
  useEffect(() => {
    if (!runtimeTargetKey) return;
    // Companion 也可能从 Android 通知栏或系统 VPN 设置改变状态。桌面端每秒只向
    // Rust 查询一次只读 ViewModel，保证这类外部变化也能在页面上及时反映。
    let disposed = false;
    let timer: number | undefined;
    const poll = async () => {
      await refreshRuntimeSerially();
      if (!disposed) timer = window.setTimeout(() => void poll(), 1_000);
    };
    timer = window.setTimeout(() => void poll(), 1_000);
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [refreshRuntimeSerially, runtimeTargetKey]);

  useEffect(() => {
    if (!runtimeData || !selectedOwner || !runtimeResponseMatches(selectedOwner, runtimeData)) return;
    if (runtimeData.state === "stopped" || runtimeData.state === "faulted") {
      lastActiveProfileIds.current.delete(selectedOwner.serial);
      return;
    }
    if (runtimeData.active_profile_id) {
      lastActiveProfileIds.current.set(selectedOwner.serial, runtimeData.active_profile_id);
    }
  }, [runtimeData, selectedOwner]);
  const [pendingTargets, setPendingTargets] = useState<Record<string, string | undefined>>({});
  const [dangerousConfirmed, setDangerousConfirmed] = useState(false);
  const selectedSerialRef = useRef<string | null | undefined>(selectedSerial);
  const runtimeOwnerKeyRef = useRef(runtimeTargetKey);
  useLayoutEffect(() => {
    selectedSerialRef.current = selectedSerial;
    runtimeOwnerKeyRef.current = runtimeTargetKey;
  }, [runtimeTargetKey, selectedSerial]);

  const busy = Boolean(selectedSerial && pendingTargets[selectedSerial]);
  const busySerials = new Set(Object.keys(pendingTargets).filter((serial) => pendingTargets[serial]));
  const inventory = packages.data ?? [];
  const selectedPackages = new Set(
    draft?.target_applications.map((item) => item.package_name) ?? [],
  );
  const visiblePackages = packageFilter ? (filteredPackages.data ?? []) : inventory;

  async function run(
    name: string,
    action: () => Promise<void>,
    serial = selectedSerial ?? "global",
  ): Promise<void> {
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
  }

  async function selectDevice(serial: string): Promise<void> {
    const target = targets.find((candidate) => candidate.serial === serial);
    setSelectedSerialOverride(serial);
    if (target?.online) {
      await callCommand(commands.androidAdbSelect(serial));
      await adb.refresh();
    }
    packages.invalidate();
    filteredPackages.invalidate();
    clearPackageFilter();
    toast(`已选择设备 ${serial}。`, { variant: "success" });
  }

  async function newProfile(): Promise<void> {
    setDraft(await callCommand(commands.deviceNetworkProfileNew()));
    setDangerousConfirmed(false);
  }

  async function openProfile(profileId: string): Promise<void> {
    setDraft(await callCommand(commands.deviceNetworkProfileGet(profileId)));
    setDangerousConfirmed(false);
  }

  function updateWeak(changes: Partial<WeakNetworkProfile>): void {
    if (!draft) return;
    setDraft({ ...draft, weak_network: { ...draft.weak_network, ...changes } });
  }

  async function applyProfileIntent(intent: AndroidProfileEditIntent): Promise<void> {
    if (!draft || !selectedSerial) return;
    setDraft(await callCommand(commands.deviceNetworkProfileApplyIntent(selectedSerial, draft, intent)));
  }

  function togglePackage(item: AndroidPackageViewModel, enabled: boolean): void {
    void run("toggle-package", () => applyProfileIntent({
      kind: "toggle_package",
      package_name: item.package_name,
      selected: enabled,
    }));
  }

  async function saveProfile(): Promise<AndroidNetworkProfile> {
    if (!draft || !selectedSerial) throw new Error("请先选择设备并新建或选择设备网络方案。");
    const saved = await callCommand(commands.deviceNetworkProfileSave(selectedSerial, draft));
    setDraft(saved);
    await profiles.refresh();
    return saved;
  }

  async function activate(operation: "start" | "apply"): Promise<void> {
    if (runtimeOwners.data === undefined || runtimeOwners.error || !selectedSerial) {
      throw new Error("运行设备列表尚未确认，请等待读取完成后重试。");
    }
    const selectedAtStart = selectedSerial;
    const ownerKeyAtStart = runtimeTargetKey;
    const ownerAtStart = selectedOwner;
    const saved = await saveProfile();
    if (
      selectedSerialRef.current !== selectedAtStart
      || runtimeOwnerKeyRef.current !== ownerKeyAtStart
    ) {
      throw new Error("设备选择或运行所有者已变化，请确认当前状态后重试。");
    }
    const result = operation === "start"
      ? await callCommand(commands.deviceNetworkStart(selectedAtStart, saved.id, dangerousConfirmed))
      : await callCommand(commands.deviceNetworkApply(
        selectedAtStart,
        ownerAtStart!.epoch,
        saved.id,
        dangerousConfirmed,
      ));
    if (result.serial === selectedAtStart) runtime.setData(result);
    runtimeOwners.invalidate();
    await Promise.all([runtimeOwners.refresh(), endpoints.refresh()]);
    toast(result.message, { variant: toneColor(result.ui_tone!) });
  }

  function clearPackageFilter(): void {
    setPackageFilterDraft("");
    setPackageFilter("");
  }

  const readErrors = [
    adb.error,
    devices.error,
    packages.error,
    filteredPackages.error,
    profiles.error,
    runtimeOwners.error,
    runtime.error,
  ].filter(Boolean).join("；");

  return (
    <section className="h-full min-h-0 overflow-hidden p-5">
      <div className="mx-auto flex h-full w-full max-w-[1680px] flex-col gap-4">
        <div>
          <h1 className="sr-only">应用网络接管</h1>
          <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
            设备端 VPN 只接管所选应用，可将指定目标透明转交代理入口，并按需实施 TCP/IP 弱网。
          </p>
        </div>

        {readErrors && (
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>设备控制读取失败</Alert.Title>
              <Alert.Description>{readErrors}</Alert.Description>
            </Alert.Content>
          </Alert>
        )}

        <div className="min-h-0 flex-1 space-y-4 overflow-auto pr-1 [scrollbar-gutter:stable]">
          <DeviceControlCard
            adb={adb.data}
            adbLoading={adb.isLoading}
            devices={devices.data ?? []}
            devicesLoading={devices.isLoading && devices.data === undefined}
            devicesReady={devices.data !== undefined}
            devicesError={devices.error}
            selectedSerial={selectedSerial}
            runtimeOwners={runtimeOwners.data ?? []}
            busySerials={busySerials}
            globalBusy={Boolean(pendingTargets.global)}
            onRefreshDevices={() => void run("devices", async () => { await devices.refresh(); }, "global")}
            onSelectDevice={(serial) => void run("select-device", () => selectDevice(serial), serial)}
            onInstall={() => void run("install", installCompanion)}
            onUpdate={() => void run("update", updateCompanion)}
            onConsent={() => void run("consent", requestVpnConsent)}
            onRefreshStatus={(owner) => void run(`status:${owner.serial}`, () => refreshOwnerStatus(owner), owner.serial)}
            onStop={(owner) => void run(`stop:${owner.serial}`, () => stopNetwork(owner), owner.serial)}
            onEmergencyRestore={(owner) => void run(`emergency:${owner.serial}`, () => emergencyRestore(owner), owner.serial)}
          />

          {displayedRuntime && (
            <Alert status={runtimeAlertStatus(displayedRuntime.ui_tone!)}>
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>
                  {displayedRuntime.state_text} · {displayedRuntime.verified ? "已验证" : "未验证"}
                </Alert.Title>
                <Alert.Description>{displayedRuntime.message}</Alert.Description>
              </Alert.Content>
            </Alert>
          )}

          <RuntimeEndpointsCard
            snapshot={endpoints.data}
            loading={endpoints.isLoading}
            error={endpoints.error}
          />

          <ProfileSelectorCard
            profiles={profiles.data ?? []}
            selectedProfileId={draft?.id}
            activeProfileId={activeProfileId}
            vpnStateText={displayedRuntime?.state_text}
            loading={profiles.isLoading}
            busy={busy}
            onNew={() => void run("new", newProfile)}
            onOpen={(profileId) => void run("open", () => openProfile(profileId))}
          />

          {!draft ? (
            profiles.data?.length ? <UnselectedProfileState /> : null
          ) : (
            <>
              <ProfileBasicsCard draft={draft} onChange={setDraft} />
              <TargetApplicationsCard
                visiblePackages={visiblePackages}
                selectedPackages={selectedPackages}
                filterDraft={packageFilterDraft}
                activeFilter={packageFilter}
                selectedSerial={selectedSerial}
                filtering={filteredPackages.isLoading}
                refreshing={Boolean(selectedSerial && pendingTargets[selectedSerial] === "refresh-packages")}
                onFilterDraftChange={setPackageFilterDraft}
                onApplyFilter={() => setPackageFilter(packageFilterDraft.trim())}
                onClearFilter={clearPackageFilter}
                onRefresh={() => void run("refresh-packages", refreshPackages)}
                onTogglePackage={togglePackage}
              />
              <ProxyRoutesCard
                draft={draft}
                listeners={workspaceListeners.listeners}
                loading={workspaceListeners.loading}
                error={workspaceListeners.error}
                onChange={setDraft}
              />
              <DestinationTargetsCard draft={draft} onChange={setDraft} />
              <BasicNetworkParametersCard weak={draft.weak_network} onUpdate={updateWeak} />
              <AdvancedNetworkCard
                weak={draft.weak_network}
                onUpdate={updateWeak}
                onApplyIntent={(intent) => void run(
                  `profile-intent:${intent.kind}`,
                  () => applyProfileIntent(intent),
                )}
              />
              <ProfileActions
                busy={busy}
                selectedSerial={selectedSerial}
                runtimeOwner={selectedOwner}
                runtimeOwnerCount={(runtimeOwners.data ?? []).length}
                runtimeOwnerReady={runtimeOwners.data !== undefined && !runtimeOwners.error}
                dangerousConfirmed={dangerousConfirmed}
                onDangerousConfirmedChange={setDangerousConfirmed}
                onSave={() => void run("save", saveAndNotify)}
                onStart={() => void run("start", () => activate("start"))}
                onApply={() => void run("apply", () => activate("apply"))}
              />
            </>
          )}
        </div>
      </div>
    </section>
  );

  async function installCompanion(): Promise<void> {
    if (!selectedSerial) return;
    await callCommand(commands.androidCompanionInstall(selectedSerial));
    toast("设备端组件安装成功。", { variant: "success" });
  }

  async function refreshPackages(): Promise<void> {
    if (!selectedSerial) return;
    const serial = selectedSerial;
    const refreshed = await callCommand(commands.androidPackageRefresh(serial));
    if (selectedSerialRef.current !== serial) return;
    packages.setData(refreshed);
    if (packageFilter) {
      filteredPackages.setData(await callCommand(commands.androidPackageQuery(serial, packageFilter)));
    }
    toast(`已从设备重新读取 ${refreshed.length} 个应用。`, { variant: "success" });
  }

  async function updateCompanion(): Promise<void> {
    if (!selectedSerial) return;
    await callCommand(commands.androidCompanionUpdate(selectedSerial));
    toast("设备端组件更新成功。", { variant: "success" });
  }

  async function requestVpnConsent(): Promise<void> {
    if (!selectedSerial) return;
    await callCommand(commands.androidVpnOpenConsent(selectedSerial));
  }

  async function emergencyRestore(owner: AndroidRuntimeOwnerViewModel): Promise<void> {
    const result = await callCommand(commands.deviceNetworkEmergencyRestore(owner.serial, owner.epoch));
    if (
      selectedSerialRef.current === owner.serial
      && runtimeOwnerKeyRef.current === runtimeOwnerQueryKey(owner)
      && runtimeResponseMatches(owner, result)
    ) {
      runtime.invalidate(false);
      runtime.setData(result);
    }
    runtimeOwners.setData((current) => clearOwnerConditionally(current ?? [], owner));
    await adb.refresh();
    await Promise.all([runtimeOwners.refresh(), endpoints.refresh()]);
  }

  async function saveAndNotify(): Promise<void> {
    await saveProfile();
    toast("设备网络方案已校验并保存。", { variant: "success" });
  }

  async function stopNetwork(owner: AndroidRuntimeOwnerViewModel): Promise<void> {
    const result = await callCommand(commands.deviceNetworkStop(owner.serial, owner.epoch));
    if (
      selectedSerialRef.current === owner.serial
      && runtimeOwnerKeyRef.current === runtimeOwnerQueryKey(owner)
      && runtimeResponseMatches(owner, result)
    ) {
      runtime.invalidate(false);
      runtime.setData(result);
    }
    runtimeOwners.setData((current) => clearOwnerConditionally(current ?? [], owner));
    await adb.refresh();
    await Promise.all([runtimeOwners.refresh(), endpoints.refresh()]);
  }

  async function refreshOwnerStatus(owner: AndroidRuntimeOwnerViewModel): Promise<void> {
    const result = await callCommand(commands.deviceNetworkStatus(owner.serial));
    if (
      selectedSerialRef.current === owner.serial
      && runtimeOwnerKeyRef.current === runtimeOwnerQueryKey(owner)
      && runtimeResponseMatches(owner, result)
    ) {
      runtime.setData(result);
    }
  }
}
