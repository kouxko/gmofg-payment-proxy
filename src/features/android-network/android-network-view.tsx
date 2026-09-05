"use client";

import type { ReactElement } from "react";
import { Alert, toast } from "@heroui/react";
import type {
  AndroidNetworkProfile,
  AndroidPackageViewModel,
  AndroidProfileEditIntent,
  AndroidRuntimeOwnerViewModel,
  WeakNetworkProfile,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand } from "@/lib/ipc/client";
import { toneColor } from "@/lib/format";
import { DeviceControlCard } from "./device-control-card";
import {
  BasicNetworkParametersCard,
} from "./network-parameter-cards";
import { NetworkMoreSettings } from "./network-more-settings";
import {
  ProfileActions,
  ProfileBasicsCard,
  ProfileSelectorCard,
  UnselectedProfileState,
} from "./profile-cards";
import { TargetApplicationsCard } from "./target-applications-card";
import { useCurrentWorkspaceListeners } from "./use-current-workspace-listeners";
import { runtimeAlertStatus } from "./android-network-types";
import {
  clearOwnerConditionally,
  runtimeOwnerQueryKey,
  runtimeResponseMatches,
} from "./android-runtime-owner-model";
import { useAndroidNetworkController } from "./use-android-network-controller";

/**
 * 页面只展示 Rust ViewModel、收集表单输入并发送用户意图。
 * 设备发现、规则校验、持久化和 VPN 状态判断均由 Rust 完成。
 */
export function AndroidNetworkView(): ReactElement {
  const controller = useAndroidNetworkController();
  const {
    adb, devices, profiles, overview, owners, targets, selectedSerial, selectedOwner,
    runtimeTargetKey, packages, packageFilterDraft, packageFilter, filteredPackages,
    runtime, displayedRuntime, activeProfileId, draft, setDraft, endpoints,
    pendingTargets, dangerousConfirmed, selectedSerialRef, runtimeOwnerKeyRef,
    setSelectedSerialOverride, setPackageFilterDraft, setPackageFilter,
    setDangerousConfirmed, run,
  } = controller;
  const workspaceListeners = useCurrentWorkspaceListeners();

  const busy = Boolean(selectedSerial && pendingTargets[selectedSerial]);
  const busySerials = new Set(Object.keys(pendingTargets).filter((serial) => pendingTargets[serial]));
  const inventory = packages.data ?? [];
  const selectedPackages = new Set(
    draft?.target_applications.map((item) => item.package_name) ?? [],
  );
  const visiblePackages = packageFilter ? (filteredPackages.data ?? []) : inventory;

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
    if (overview.data === undefined || overview.error || !selectedSerial) {
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
    overview.invalidate();
    await Promise.all([overview.refresh(), endpoints.refresh()]);
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
    overview.error,
    runtime.error,
  ].filter(Boolean).join("；");

  return (
    <section className="h-full min-h-0 overflow-hidden p-5">
      <div className="mx-auto flex h-full w-full max-w-[1680px] flex-col gap-4">
        <div>
          <h1 className="sr-only">应用网络接管</h1>
          <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
            选择设备和目标应用后即可单独运行弱网；需要时再接入代理调试或展开专家设置。
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
            runtimeOwners={owners}
            maxRuntimeOwners={overview.data?.max_runtime_owners}
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
              <BasicNetworkParametersCard key={draft.id} weak={draft.weak_network} onUpdate={updateWeak} />
              <NetworkMoreSettings
                draft={draft}
                listeners={workspaceListeners.listeners}
                listenersLoading={workspaceListeners.loading}
                listenersError={workspaceListeners.error}
                endpointsSnapshot={endpoints.data}
                endpointsLoading={endpoints.isLoading}
                endpointsError={endpoints.error}
                onChange={setDraft}
                onUpdateWeak={updateWeak}
                onApplyIntent={(intent) => void run(
                  `profile-intent:${intent.kind}`,
                  () => applyProfileIntent(intent),
                )}
              />
              <ProfileActions
                busy={busy}
                selectedSerial={selectedSerial}
                runtimeOwner={selectedOwner}
                runtimeOwnerCount={owners.length}
                maxRuntimeOwners={overview.data?.max_runtime_owners}
                canStartNewOwner={overview.data?.can_start_new_owner ?? false}
                runtimeOwnerReady={overview.data !== undefined && !overview.error}
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
    overview.setData((current) => current
      ? { ...current, owners: clearOwnerConditionally(current.owners, owner) }
      : current);
    await adb.refresh();
    await Promise.all([overview.refresh(), endpoints.refresh()]);
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
    overview.setData((current) => current
      ? { ...current, owners: clearOwnerConditionally(current.owners, owner) }
      : current);
    await adb.refresh();
    await Promise.all([overview.refresh(), endpoints.refresh()]);
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
