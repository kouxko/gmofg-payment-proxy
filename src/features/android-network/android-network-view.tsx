"use client";

import { useState, type ReactElement } from "react";
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
  EmptyProfileState,
  ProfileActions,
  ProfileBasicsCard,
  ProfileSelectorCard,
} from "./profile-cards";
import { TargetApplicationsCard } from "./target-applications-card";
import { ProxyRoutesCard } from "./proxy-routes-card";
import { useCurrentWorkspaceListeners } from "./use-current-workspace-listeners";

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
    `android-runtime:${selectedSerial ?? "none"}`,
    () => callCommand(commands.deviceNetworkStatus()),
    undefined,
    { enabled: Boolean(selectedSerial) },
  );
  const [draft, setDraft] = useState<AndroidNetworkProfile>();
  const [pending, setPending] = useState<string>();
  const [dangerousConfirmed, setDangerousConfirmed] = useState(false);

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
    runtime.invalidate();
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
    const saved = await saveProfile();
    const result = operation === "start"
      ? await callCommand(commands.deviceNetworkStart(saved.id, dangerousConfirmed))
      : await callCommand(commands.deviceNetworkApply(saved.id, dangerousConfirmed));
    runtime.setData(result);
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
            busy={busy}
            onRefreshDevices={() => void run("devices", async () => { await devices.refresh(); })}
            onSelectDevice={(serial) => void run("select-device", () => selectDevice(serial))}
            onInstall={() => void run("install", installCompanion)}
            onUpdate={() => void run("update", updateCompanion)}
            onConsent={() => void run("consent", requestVpnConsent)}
            onRefreshStatus={() => void run("status", async () => { await runtime.refresh(); })}
            onEmergencyRestore={() => void run("emergency", emergencyRestore)}
          />

          {runtime.data && (
            <Alert status={runtimeAlertStatus(runtime.data.ui_tone!)}>
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>
                  {runtime.data.state_text} · {runtime.data.verified ? "已验证" : "未验证"}
                </Alert.Title>
                <Alert.Description>{runtime.data.message}</Alert.Description>
              </Alert.Content>
            </Alert>
          )}

          <ProfileSelectorCard
            profiles={profiles.data ?? []}
            selectedProfileId={draft?.id}
            loading={profiles.isLoading}
            busy={busy}
            onNew={() => void run("new", newProfile)}
            onOpen={(profileId) => void run("open", () => openProfile(profileId))}
          />

          {!draft ? (
            <EmptyProfileState busy={busy} onNew={() => void run("new-empty", newProfile)} />
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
                onFilterDraftChange={setPackageFilterDraft}
                onApplyFilter={() => setPackageFilter(packageFilterDraft.trim())}
                onClearFilter={clearPackageFilter}
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
                dangerousConfirmed={dangerousConfirmed}
                onDangerousConfirmedChange={setDangerousConfirmed}
                onSave={() => void run("save", saveAndNotify)}
                onStart={() => void run("start", () => activate("start"))}
                onApply={() => void run("apply", () => activate("apply"))}
                onStop={() => void run("stop", stopNetwork)}
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

  async function updateCompanion(): Promise<void> {
    await callCommand(commands.androidCompanionUpdate());
    toast("设备端组件更新成功。", { variant: "success" });
  }

  async function requestVpnConsent(): Promise<void> {
    runtime.setData(await callCommand(commands.androidVpnOpenConsent()));
  }

  async function emergencyRestore(): Promise<void> {
    runtime.setData(await callCommand(commands.deviceNetworkEmergencyRestore()));
  }

  async function saveAndNotify(): Promise<void> {
    await saveProfile();
    toast("设备网络方案已由 Rust 校验并保存。", { variant: "success" });
  }

  async function stopNetwork(): Promise<void> {
    runtime.setData(await callCommand(commands.deviceNetworkStop()));
  }
}

function runtimeAlertStatus(tone: UiTone): "danger" | "success" | "warning" | "accent" {
  if (tone === "danger") return "danger";
  if (tone === "positive") return "success";
  if (tone === "warning") return "warning";
  return "accent";
}
