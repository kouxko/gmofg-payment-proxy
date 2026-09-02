import { useRef, useState } from "react";
import { Alert, AlertDialog, Button, Modal } from "@heroui/react";
import { Xmark } from "@gravity-ui/icons";
import type {
  ProtocolPackageDetailViewModel,
  ProtocolPackageGroupViewModel,
  ProtocolPackageVersionViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import { ProtocolPackageDetail } from "./protocol-package-detail";
import { isProtocolPackageVersion, protocolPackageDetailError } from "./protocol-package-model";
import { ProtocolPackageVersionList } from "./protocol-package-version-list";

export function ProtocolPackageDialog({
  group,
  selectedVersion,
  isOpen,
  announcement,
  onVersionChange,
  onVersionUpdated,
  onVersionDeleted,
  onOpenChange,
}: {
  group?: ProtocolPackageGroupViewModel;
  selectedVersion?: ProtocolPackageVersionViewModel;
  isOpen: boolean;
  announcement?: string;
  onVersionChange: (version: ProtocolPackageVersionViewModel) => void;
  onVersionUpdated: (version: ProtocolPackageVersionViewModel) => void;
  onVersionDeleted: (version: ProtocolPackageVersionViewModel) => void;
  onOpenChange: (open: boolean) => void;
}) {
  const mutationLock = useRef(false);
  const [lifecycle, setLifecycle] = useState<LifecycleState>({ kind: "idle" });
  const packageRef = selectedVersion?.package;
  const packageKey = packageRef ? `${packageRef.id}\u0000${packageRef.version}` : "";
  const visibleLifecycle = "packageKey" in lifecycle && lifecycle.packageKey !== packageKey
    ? { kind: "idle" } as const
    : lifecycle;
  const writePending = visibleLifecycle.kind === "enabling"
    || visibleLifecycle.kind === "disabling"
    || visibleLifecycle.kind === "restarting"
    || visibleLifecycle.kind === "deleting";
  const detail = useIpcQuery<ProtocolPackageDetailViewModel>(
    `protocol-package-detail:${packageRef?.id ?? ""}@${packageRef?.version ?? ""}`,
    () => callCommand(commands.protocolPackageDetail({ id: packageRef!.id, version: packageRef!.version })),
    undefined,
    { enabled: isOpen && packageRef != null },
  );
  const responseError = detail.data !== undefined && packageRef
    ? protocolPackageDetailError(detail.data, packageRef)
    : undefined;
  const visibleDetail = responseError
    ? { data: undefined, error: responseError, isLoading: false }
    : detail;
  useAppEventRefresh(
    ["protocol_package_catalog_changed", "snapshot_required"],
    detail.refresh,
  );
  const currentVersion = responseError ? selectedVersion : detail.data?.version ?? selectedVersion;

  async function enableVersion() {
    if (!currentVersion || currentVersion.enabled || mutationLock.current) return;
    mutationLock.current = true;
    setLifecycle({ kind: "enabling", packageKey });
    try {
      const enabled = await callCommand(commands.protocolPackageEnable(currentVersion.package));
      if (!isProtocolPackageVersion(enabled)
        || enabled.package.id !== currentVersion.package.id
        || enabled.package.version !== currentVersion.package.version
        || enabled.enabled !== true
        || enabled.validation.state !== "valid") {
        setLifecycle({ kind: "enable-error", packageKey, message: "协议包启用结果不完整，请刷新列表后重试。" });
        return;
      }
      if (detail.data) detail.setData({ ...detail.data, version: enabled });
      onVersionUpdated(enabled);
      setLifecycle({ kind: "idle" });
    } catch (reason) {
      setLifecycle({ kind: "enable-error", packageKey, message: errorMessage(reason) });
    } finally {
      mutationLock.current = false;
    }
  }

  async function disableVersion() {
    if (!currentVersion || !currentVersion.enabled || mutationLock.current) return;
    mutationLock.current = true;
    setLifecycle({ kind: "disabling", packageKey });
    try {
      const disabled = await callCommand(commands.protocolPackageDisable(currentVersion.package));
      if (!isProtocolPackageVersion(disabled)
        || disabled.package.id !== currentVersion.package.id
        || disabled.package.version !== currentVersion.package.version
        || disabled.package_source.type !== currentVersion.package_source.type
        || disabled.enabled) {
        setLifecycle({ kind: "disable-error", packageKey, message: "协议包停用结果不完整，请刷新列表后重试。" });
        return;
      }
      if (detail.data) detail.setData({ ...detail.data, version: disabled });
      onVersionUpdated(disabled);
      setLifecycle({ kind: "idle" });
    } catch (reason) {
      setLifecycle({ kind: "disable-error", packageKey, message: errorMessage(reason) });
    } finally {
      mutationLock.current = false;
    }
  }

  async function restartVersion() {
    if (!currentVersion || !currentVersion.enabled || currentVersion.package_source.type !== "managed" || mutationLock.current) return;
    mutationLock.current = true;
    setLifecycle({ kind: "restarting", packageKey });
    try {
      const restarted = await callCommand(commands.protocolPackageRestart(currentVersion.package));
      if (!isProtocolPackageVersion(restarted)
        || restarted.package.id !== currentVersion.package.id
        || restarted.package.version !== currentVersion.package.version
        || restarted.package_source.type !== "managed") {
        setLifecycle({ kind: "restart-error", packageKey, message: "本地软件包重启结果不完整，请刷新列表后重试。" });
        return;
      }
      if (detail.data) detail.setData({ ...detail.data, version: restarted });
      onVersionUpdated(restarted);
      await detail.refresh();
      setLifecycle({ kind: "idle" });
    } catch (reason) {
      setLifecycle({ kind: "restart-error", packageKey, message: errorMessage(reason) });
    } finally {
      mutationLock.current = false;
    }
  }

  async function deleteVersion() {
    if (!currentVersion || mutationLock.current || detail.data?.usages.length !== 0) return;
    mutationLock.current = true;
    setLifecycle({ kind: "deleting", packageKey });
    try {
      const result = await callCommand(commands.protocolPackageDelete(currentVersion.package));
      const resultError = protocolPackageDeleteResultError(result, packageKey.replace("\u0000", "@"));
      if (resultError) {
        setLifecycle({ kind: "delete-error", packageKey, message: resultError });
        return;
      }
      setLifecycle({ kind: "idle" });
      onVersionDeleted(currentVersion);
    } catch (reason) {
      setLifecycle({ kind: "delete-error", packageKey, message: errorMessage(reason) });
    } finally {
      mutationLock.current = false;
    }
  }

  const deleteBlockedReason = detail.data && detail.data.usages.length > 0
    ? `请先修改或删除：${detail.data.usages.map((usage) => `${usage.workspace_name} / ${usage.listener_name}`).join("；")}`
    : undefined;

  return (
    <>
    <Modal isOpen={isOpen} onOpenChange={(open) => {
      if (!open && writePending) return;
      if (!open) setLifecycle({ kind: "idle" });
      onOpenChange(open);
    }}>
      <Button className="hidden" aria-hidden="true">打开协议包详情</Button>
      <Modal.Backdrop isDismissable={!writePending}>
        <Modal.Container size="cover" scroll="inside">
          <Modal.Dialog>
            <Modal.Header className="items-start gap-1 pr-12 text-left">
              <Modal.Heading className="max-w-full break-words text-left text-lg font-semibold">
                {group?.name || group?.id || "协议包详情"}
              </Modal.Heading>
              <p className="max-w-full break-all text-left font-mono text-xs text-[var(--telemetry-muted)]">{group?.id ?? ""}</p>
              <Modal.CloseTrigger aria-label="关闭协议包详情" isDisabled={writePending}>
                <Xmark className="size-4" />
              </Modal.CloseTrigger>
            </Modal.Header>
            <Modal.Body className="min-h-0 overflow-y-auto">
              {announcement && <p role="status" className="mb-4 text-sm text-success">{announcement}</p>}
              <div className="grid min-w-0 gap-5 lg:grid-cols-[14rem_minmax(0,1fr)]">
                <ProtocolPackageVersionList
                  versions={group ? group.versions : []}
                  selectedVersion={selectedVersion?.package.version}
                  isDisabled={writePending}
                  onSelect={(version) => {
                    setLifecycle({ kind: "idle" });
                    onVersionChange(version);
                  }}
                />
                <ProtocolPackageDetail
                  detail={visibleDetail}
                  enablePending={visibleLifecycle.kind === "enabling"}
                  enableError={visibleLifecycle.kind === "enable-error" ? visibleLifecycle.message : undefined}
                  disablePending={visibleLifecycle.kind === "disabling"}
                  disableError={visibleLifecycle.kind === "disable-error" ? visibleLifecycle.message : undefined}
                  restartPending={visibleLifecycle.kind === "restarting"}
                  restartError={visibleLifecycle.kind === "restart-error" ? visibleLifecycle.message : undefined}
                  deleteBlockedReason={deleteBlockedReason}
                  onEnable={() => void enableVersion()}
                  onDisable={() => void disableVersion()}
                  onRestart={() => void restartVersion()}
                  onRequestDelete={() => setLifecycle({ kind: "delete-confirm", packageKey })}
                />
              </div>
            </Modal.Body>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
    <AlertDialog
      isOpen={visibleLifecycle.kind === "delete-confirm" || visibleLifecycle.kind === "deleting" || visibleLifecycle.kind === "delete-error"}
      onOpenChange={(open) => {
        if (visibleLifecycle.kind === "deleting") return;
        setLifecycle(open ? { kind: "delete-confirm", packageKey } : { kind: "idle" });
      }}
    >
      <Button className="hidden" aria-hidden="true">打开协议包删除确认</Button>
      <AlertDialog.Backdrop isDismissable={visibleLifecycle.kind !== "deleting"}>
        <AlertDialog.Container>
          <AlertDialog.Dialog>
            <AlertDialog.Header>
              <AlertDialog.Heading>删除 {selectedVersion?.name ?? "协议包"} {selectedVersion?.package.version ?? ""}？</AlertDialog.Heading>
            </AlertDialog.Header>
            <AlertDialog.Body className="space-y-3">
              <p>{selectedVersion?.package_source.type === "managed"
                ? "此操作会永久删除该精确版本及其本地文件。仍有入口引用时不能删除。"
                : "此操作会永久删除该精确版本的元数据。若远端调试连接仍在线，Proxy 会先关闭对应连接；之后重新注册将视为首次连接并默认停用。"}</p>
              {visibleLifecycle.kind === "delete-error" ? (
                <Alert status="danger">
                  <Alert.Indicator />
                  <Alert.Content>
                    <Alert.Title>协议包删除失败</Alert.Title>
                    <Alert.Description>{visibleLifecycle.message}</Alert.Description>
                  </Alert.Content>
                </Alert>
              ) : null}
            </AlertDialog.Body>
            <AlertDialog.Footer>
              <Button slot="close" variant="outline" isDisabled={visibleLifecycle.kind === "deleting"}>取消</Button>
              <Button variant="danger" isDisabled={visibleLifecycle.kind === "deleting"} onPress={() => void deleteVersion()}>
                {visibleLifecycle.kind === "deleting" ? "正在删除…" : "确认删除"}
              </Button>
            </AlertDialog.Footer>
          </AlertDialog.Dialog>
        </AlertDialog.Container>
      </AlertDialog.Backdrop>
    </AlertDialog>
    </>
  );
}

type LifecycleState =
  | { kind: "idle" }
  | { kind: "enabling"; packageKey: string }
  | { kind: "enable-error"; packageKey: string; message: string }
  | { kind: "disabling"; packageKey: string }
  | { kind: "disable-error"; packageKey: string; message: string }
  | { kind: "restarting"; packageKey: string }
  | { kind: "restart-error"; packageKey: string; message: string }
  | { kind: "delete-confirm"; packageKey: string }
  | { kind: "deleting"; packageKey: string }
  | { kind: "delete-error"; packageKey: string; message: string };

function protocolPackageDeleteResultError(value: unknown, expectedEntity: string): string | undefined {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return "协议包删除结果不完整，请刷新列表后重试。";
  }
  const result = value as Record<string, unknown>;
  const expectedKeys = ["success", "cancelled", "message", "ui_tone", "entity_id", "revision", "requires_restart"];
  if (Object.keys(result).length !== expectedKeys.length
    || !expectedKeys.every((key) => Object.hasOwn(result, key))
    || result.success !== true
    || result.cancelled !== false
    || typeof result.message !== "string"
    || result.message.length === 0
    || result.ui_tone !== "positive"
    || result.entity_id !== expectedEntity
    || result.revision !== null
    || result.requires_restart !== false) {
    return "协议包删除结果不完整，请刷新列表后重试。";
  }
  return undefined;
}
