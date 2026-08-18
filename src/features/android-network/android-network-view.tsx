"use client";

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactElement,
} from "react";
import { Alert, toast } from "@heroui/react";
import type {
  AndroidNetworkProfile,
  AndroidPackageViewModel,
  AndroidProfileEditIntent,
  UiTone,
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
import {
  isForeignRuntimeOwner,
  runtimeOwnerQueryKey,
} from "./android-runtime-owner-model";

/**
 * 页面只展示 Rust ViewModel、收集表单输入并发送用户意图。
 * 设备发现、规则校验、持久化和 VPN 状态判断均由 Rust 完成。
 */
export function AndroidNetworkView(): ReactElement {
  const adb = useIpcQuery("android-adb", () => callCommand(commands.androidAdbGet()));
  const devices = useIpcQuery("android-devices", () => callCommand(commands.androidDeviceList()), []);
  const profiles = useIpcQuery(
    "android-profiles",
    () => callCommand(commands.deviceNetworkProfileList()),
    [],
  );
  const workspaceListeners = useCurrentWorkspaceListeners();
  const selectedSerial = adb.data?.selected_serial;
  const runtimeOwner = useIpcQuery(
    "android-runtime-owner",
    () => callCommand(commands.deviceNetworkRuntimeOwner()),
    undefined,
  );
  const runtimeOwnerData = runtimeOwner.data;
  const refreshRuntimeOwner = runtimeOwner.refresh;
  const runtimeTargetSerial = runtimeOwnerData?.serial;
  const runtimeTargetKey = runtimeOwnerQueryKey(runtimeOwnerData);
  const packages = useIpcQuery(
    `android-packages:${selectedSerial ?? "none"}`,
    () => callCommand(commands.androidPackageList()),
    [],
    { enabled: Boolean(selectedSerial) },
  );
  const [packageFilterDraft, setPackageFilterDraft] = useState("");
  const [packageFilter, setPackageFilter] = useState("");
  const filteredPackages = useIpcQuery(
    `android-packages:${selectedSerial ?? "none"}:filter:${packageFilter}`,
    () => callCommand(commands.androidPackageQuery(packageFilter)),
    [],
    { enabled: Boolean(selectedSerial && packageFilter) },
  );
  const runtime = useIpcQuery(
    `android-runtime:${runtimeTargetKey ?? "none"}`,
    () => callCommand(commands.deviceNetworkStatus()),
    undefined,
    { enabled: Boolean(runtimeTargetKey) },
  );
  const runtimeData = runtime.data;
  const refreshRuntime = runtime.refresh;
  const [draft, setDraft] = useState<AndroidNetworkProfile>();
  const endpoints = useIpcQuery(
    `android-endpoints:${draft?.id ?? "none"}:${runtimeTargetKey ?? "none"}`,
    () => callCommand(commands.deviceNetworkEndpoints(draft?.id ?? null)),
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
  const refreshRuntimeEvent = useCallback(async (): Promise<void> => {
    await Promise.all([
      refreshRuntimeOwner(),
      refreshRuntimeSerially(),
      refreshEndpoints(),
    ]);
  }, [refreshEndpoints, refreshRuntimeOwner, refreshRuntimeSerially]);
  useAppEventRefresh(
    ["android_vpn_status_changed"],
    refreshRuntimeEvent,
    {
      paused: !runtimeTargetSerial,
      entityId: runtimeTargetSerial ?? undefined,
    },
  );
  // 设备切换时查询结果会短暂保留上一台设备的数据。活动方案缓存必须按 serial 隔离，
  // 否则新设备会错误显示上一台设备正在运行的方案。
  const lastActiveProfileIds = useRef(new Map<string, string>());
  const displayedRuntime = runtimeData?.serial === runtimeTargetSerial ? runtimeData : undefined;
  const activeProfileId = displayedRuntime?.active_profile_id
    ?? (displayedRuntime?.state === "running" && runtimeTargetSerial
      ? lastActiveProfileIds.current.get(runtimeTargetSerial)
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
    if (!runtimeData || !runtimeTargetSerial || runtimeData.serial !== runtimeTargetSerial) return;
    if (runtimeData.state === "stopped" || runtimeData.state === "faulted") {
      lastActiveProfileIds.current.delete(runtimeTargetSerial);
      return;
    }
    if (runtimeData.active_profile_id) {
      lastActiveProfileIds.current.set(runtimeTargetSerial, runtimeData.active_profile_id);
    }
  }, [runtimeData, runtimeTargetSerial]);
  const [pending, setPending] = useState<string>();
  const [dangerousConfirmed, setDangerousConfirmed] = useState(false);
  const selectedSerialRef = useRef<string | null | undefined>(selectedSerial);
  const runtimeOwnerKeyRef = useRef(runtimeTargetKey);
  useLayoutEffect(() => {
    selectedSerialRef.current = selectedSerial;
    runtimeOwnerKeyRef.current = runtimeTargetKey;
  }, [runtimeTargetKey, selectedSerial]);

  const busy = Boolean(pending);
  const inventory = packages.data ?? [];
  const selectedPackages = new Set(
    draft?.target_applications.map((item) => item.package_name) ?? [],
  );
  const visiblePackages = packageFilter ? (filteredPackages.data ?? []) : inventory;

  async function run(name: string, action: () => Promise<void>): Promise<void> {
    if (pending) return;
    setPending(name);
    try {
      await action();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPending(undefined);
    }
  }

  async function selectDevice(serial: string): Promise<void> {
    await callCommand(commands.androidAdbSelect(serial));
    await adb.refresh();
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
    if (!draft) return;
    setDraft(await callCommand(commands.deviceNetworkProfileApplyIntent(draft, intent)));
  }

  function togglePackage(item: AndroidPackageViewModel, enabled: boolean): void {
    void run("toggle-package", () => applyProfileIntent({
      kind: "toggle_package",
      package_name: item.package_name,
      selected: enabled,
    }));
  }

  async function saveProfile(): Promise<AndroidNetworkProfile> {
    if (!draft) throw new Error("请先新建或选择设备网络方案。");
    const saved = await callCommand(commands.deviceNetworkProfileSave(draft));
    setDraft(saved);
    await profiles.refresh();
    return saved;
  }

  async function activate(operation: "start" | "apply"): Promise<void> {
    if (runtimeOwner.isLoading || runtimeOwner.error) {
      throw new Error("实际运行设备尚未确认，请等待读取完成后重试。");
    }
    const ownerSerial = runtimeOwnerData?.serial;
    if (ownerSerial && isForeignRuntimeOwner(selectedSerial, ownerSerial)) {
      throw new Error(`请先停止实际运行设备 ${ownerSerial}。`);
    }
    const selectedAtStart = selectedSerial;
    const ownerKeyAtStart = runtimeTargetKey;
    const saved = await saveProfile();
    if (
      selectedSerialRef.current !== selectedAtStart
      || runtimeOwnerKeyRef.current !== ownerKeyAtStart
    ) {
      throw new Error("设备选择或运行所有者已变化，请确认当前状态后重试。");
    }
    const result = operation === "start"
      ? await callCommand(commands.deviceNetworkStart(saved.id, dangerousConfirmed))
      : await callCommand(commands.deviceNetworkApply(saved.id, dangerousConfirmed));
    runtime.setData(result);
    runtimeOwner.invalidate();
    await Promise.all([runtimeOwner.refresh(), endpoints.refresh()]);
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
    runtimeOwner.error,
    runtime.error,
  ].filter(Boolean).join("；");

  return (
    <section className="h-full min-h-0 overflow-hidden p-5">
      <div className="mx-auto flex h-full w-full max-w-[1680px] flex-col gap-4">
        <div>
          <h1 className="text-2xl font-semibold">应用网络接管</h1>
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
            devicesLoading={devices.isLoading}
            selectedSerial={selectedSerial}
            runtimeOwner={runtimeOwnerData}
            busy={busy}
            onRefreshDevices={() => void run("devices", async () => { await devices.refresh(); })}
            onSelectDevice={(serial) => void run("select-device", () => selectDevice(serial))}
            onInstall={() => void run("install", installCompanion)}
            onUpdate={() => void run("update", updateCompanion)}
            onConsent={() => void run("consent", requestVpnConsent)}
            onRefreshStatus={() => void run("status", refreshRuntimeSerially)}
            onStop={() => void run("stop", stopNetwork)}
            onEmergencyRestore={() => void run("emergency", emergencyRestore)}
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
                refreshing={pending === "refresh-packages"}
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
                runtimeOwnerSerial={runtimeOwnerData?.serial}
                runtimeOwnerReady={!runtimeOwner.isLoading && !runtimeOwner.error}
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
    await callCommand(commands.androidCompanionInstall());
    toast("设备端组件安装成功。", { variant: "success" });
  }

  async function refreshPackages(): Promise<void> {
    const refreshed = await callCommand(commands.androidPackageRefresh());
    packages.setData(refreshed);
    if (packageFilter) {
      filteredPackages.setData(await callCommand(commands.androidPackageQuery(packageFilter)));
    }
    toast(`已从设备重新读取 ${refreshed.length} 个应用。`, { variant: "success" });
  }

  async function updateCompanion(): Promise<void> {
    await callCommand(commands.androidCompanionUpdate());
    toast("设备端组件更新成功。", { variant: "success" });
  }

  async function requestVpnConsent(): Promise<void> {
    await callCommand(commands.androidVpnOpenConsent());
  }

  async function emergencyRestore(): Promise<void> {
    const result = await callCommand(commands.deviceNetworkEmergencyRestore());
    runtime.invalidate(false);
    runtime.setData(result);
    runtimeOwner.invalidate();
    runtimeOwner.setData(null);
    await adb.refresh();
    await Promise.all([runtimeOwner.refresh(), endpoints.refresh()]);
  }

  async function saveAndNotify(): Promise<void> {
    await saveProfile();
    toast("设备网络方案已校验并保存。", { variant: "success" });
  }

  async function stopNetwork(): Promise<void> {
    const result = await callCommand(commands.deviceNetworkStop());
    runtime.invalidate(false);
    runtime.setData(result);
    runtimeOwner.invalidate();
    runtimeOwner.setData(null);
    await adb.refresh();
    await Promise.all([runtimeOwner.refresh(), endpoints.refresh()]);
  }
}

function runtimeAlertStatus(tone: UiTone): "danger" | "success" | "warning" | "accent" {
  if (tone === "danger") return "danger";
  if (tone === "positive") return "success";
  if (tone === "warning") return "warning";
  return "accent";
}
