"use client";

/**
 * 系统设置的草稿编辑页面。
 *
 * 本页只管理与具体代理入口无关的全局超时、容量和应用策略。监听地址、端口、
 * 上游、TLS 与入口启停统一由“入口配置”负责，避免同一网络参数出现两处入口。
 */

import { useMemo, useState } from "react";
import { Alert, AlertDialog, Button, toast } from "@heroui/react";
import { ArrowRotateLeft, FloppyDisk } from "@gravity-ui/icons";
import type {
  FieldValidationViewModel,
  SettingsDraft,
  SettingsViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import {
  appErrorViewModel,
  callCommand,
  errorMessage,
} from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import { ApplicationDataResetDialog } from "./application-data-reset-dialog";
import { SettingsEditorTabs } from "./settings-editor-tabs";

export function SettingsView() {
  const settings = useIpcQuery<SettingsViewModel>("settings-get", () =>
    callCommand(commands.settingsGet()),
  );
  useAppEventRefresh(
    ["settings_changed", "snapshot_required"],
    settings.refresh,
  );
  const [draftState, setDraftState] = useState<SettingsDraft>();
  const [validation, setValidation] = useState<FieldValidationViewModel>();
  const [pendingAction, setPendingAction] = useState<"save">();
  const [resetDialogOpen, setResetDialogOpen] = useState(false);
  const [resetPending, setResetPending] = useState(false);
  const [savedRequiresRestart, setSavedRequiresRestart] = useState(false);
  const draft = draftState ?? settings.data?.stored;
  const draftDirty = useMemo(
    () => Boolean(draft && settings.data && JSON.stringify(draft) !== JSON.stringify(settings.data.stored)),
    [draft, settings.data],
  );
  const writePending = pendingAction != null || resetPending;
  const fieldError = (field: string) =>
    validation?.field_errors[field]?.join("；");
  const mappedFields = new Set([
    "connect_timeout_seconds",
    "write_timeout_seconds",
    "read_timeout_seconds",
    "max_sessions",
    "max_memory_bytes",
    "max_body_bytes",
    "rewrite_host",
  ]);
  const globalErrors = Object.entries(validation?.field_errors ?? {})
    .filter(([field]) => !mappedFields.has(field))
    .flatMap(([, errors]) => errors);

  function setDraft(next: SettingsDraft | undefined) {
    // 用户继续编辑后，旧校验结果不再可信，必须清除并重新请求 Rust 校验。
    setDraftState(next);
    setValidation(undefined);
  }

  async function save() {
    if (!draft || writePending) return;
    setPendingAction("save");
    try {
      const checked = await callCommand(commands.settingsValidate(draft));
      setValidation(checked);
      if (!checked.valid) {
        toast(Object.values(checked.field_errors).flat().join("；") || "设置校验失败。", { variant: "danger" });
        return;
      }
      setValidation(undefined);
      const result = await callCommand(commands.settingsSave(draft));
      toast(
        result.requires_restart
          ? result.restart_reason ?? "设置已保存，需要重新打开应用后生效。"
          : "设置已保存并生效。",
        { variant: result.requires_restart ? "warning" : "success" },
      );
      settings.setData(result);
      setSavedRequiresRestart(result.requires_restart);
      setDraft(result.stored);
    } catch (reason) {
      const appError = appErrorViewModel(reason);
      if (appError) {
        setValidation({
          valid: false,
          field_errors: appError.field_errors,
          warnings: [],
        });
      }
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPendingAction(undefined);
    }
  }

  async function resetDefaults() {
    // 恢复默认值只加载草稿，不自动覆盖已保存配置，仍需用户显式保存。
    if (writePending) return;
    setResetPending(true);
    try {
      const result = await callCommand(commands.settingsResetDefaults(true));
      setDraft(result);
      setValidation(undefined);
      toast("已载入默认设置草稿，尚未保存。", { variant: "accent" });
      setResetDialogOpen(false);
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setResetPending(false);
    }
  }

  if (settings.error) {
    return (
      <div className="grid h-full place-items-center p-5">
        <Alert status="danger" className="max-w-xl">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>系统设置读取失败</Alert.Title>
            <Alert.Description>{settings.error}</Alert.Description>
          </Alert.Content>
          <Button
            size="sm"
            variant="outline"
            onPress={() => void settings.refresh()}
          >
            重试
          </Button>
        </Alert>
      </div>
    );
  }

  if (!draft || !settings.data) {
    return (
      <div className="grid h-full place-items-center text-sm text-[var(--telemetry-muted)]">
        正在读取系统设置…
      </div>
    );
  }

  return (
    <section className="flex h-full flex-col">
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-5">
        <SettingsEditorTabs
          draft={draft}
          fieldError={fieldError}
          onDraftChange={setDraft}
          isDisabled={writePending}
        />
        {globalErrors.length > 0 && (
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>设置无法保存</Alert.Title>
              <Alert.Description>{globalErrors.join("；")}</Alert.Description>
            </Alert.Content>
          </Alert>
        )}
      </div>

      <footer className="flex h-16 items-center border-t border-[var(--telemetry-line)] px-5">
        <AlertDialog
          isOpen={resetDialogOpen}
          onOpenChange={(open) => {
            if (!open && resetPending) return;
            setResetDialogOpen(open);
          }}
        >
          <Button variant="outline" isDisabled={writePending}>
            <ArrowRotateLeft className="size-4" />
            恢复默认值
          </Button>
          <AlertDialog.Backdrop>
            <AlertDialog.Container>
              <AlertDialog.Dialog>
                <AlertDialog.Header>
                  <AlertDialog.Heading>恢复默认设置草稿？</AlertDialog.Heading>
                </AlertDialog.Header>
                <AlertDialog.Body>
                  此操作只载入默认草稿，仍需点击“保存设置”才会写入。
                </AlertDialog.Body>
                <AlertDialog.Footer>
                  <Button
                    slot="close"
                    variant="outline"
                    isDisabled={resetPending}
                  >
                    取消
                  </Button>
                  <Button
                    variant="danger"
                    isDisabled={resetPending}
                    onPress={() => void resetDefaults()}
                  >
                    {resetPending ? "正在恢复…" : "确认恢复"}
                  </Button>
                </AlertDialog.Footer>
              </AlertDialog.Dialog>
            </AlertDialog.Container>
          </AlertDialog.Backdrop>
        </AlertDialog>
        <ApplicationDataResetDialog isDisabled={writePending} />
        <span className="ml-4 text-sm text-[var(--telemetry-muted)]">
          {draftDirty ? "有未保存更改" : savedRequiresRestart || settings.data.requires_restart ? "重启后生效" : "已保存"}
        </span>
        <div className="ml-auto flex gap-3">
          <Button
            variant="outline"
            isDisabled={writePending}
            onPress={() => setDraft(settings.data?.stored)}
          >
            放弃更改
          </Button>
          <Button
            variant="outline"
            isDisabled={!settings.data.can_write || writePending}
            onPress={() => void save()}
          >
            <FloppyDisk className="size-4" />
            {pendingAction === "save" ? "正在保存…" : "保存设置"}
          </Button>
        </div>
      </footer>
    </section>
  );
}
