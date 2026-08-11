"use client";

/**
 * 本机调试 Root CA 与代理服务端证书管理页面。
 *
 * 所有证书读取、生成、验证、文件对话框和密钥保护都由 Rust 执行。前端绝不接触
 * 私钥字节。某条监听启用固定 Server 后使用的 Server CA 与可选 mTLS 客户端身份，统一在
 * “代理入口”页面按监听导入、选择并测试，避免全局证书误用于其他 Server。
 */

import { useState } from "react";
import { Alert, Button, toast } from "@heroui/react";
import type {
  CertificateOverviewViewModel,
  FieldValidationViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { toneColor } from "@/lib/format";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";
import {
  CertificateOverviewSection,
  type CertificatePendingAction,
} from "./certificate-overview-section";
import { CertificateResetSection } from "./certificate-reset-section";
import { CertificateValidationSection } from "./certificate-validation-section";

export function CertificatesView() {
  const { navigate } = useWorkspaceNavigation();
  const overview =
    useIpcQuery<CertificateOverviewViewModel>("certificate-overview", () =>
      callCommand(commands.certificateOverview()),
    );
  const settings = useIpcQuery("certificate-settings", () =>
    callCommand(commands.settingsGet()),
  );
  useAppEventRefresh(
    ["certificate_status_changed", "snapshot_required"],
    overview.refresh,
  );
  useAppEventRefresh(
    ["settings_changed", "snapshot_required"],
    settings.refresh,
  );
  const [pendingAction, setPendingAction] =
    useState<CertificatePendingAction>();
  const [resetCaOpen, setResetCaOpen] = useState(false);
  const [resetCaPending, setResetCaPending] = useState(false);
  const [validation, setValidation] = useState<FieldValidationViewModel>();
  const leafSans = settings.data?.stored.leaf_sans;
  const writePending = pendingAction != null || resetCaPending;
  const localItems = (overview.data?.items ?? []).filter(
    (item) => item.kind === "local_root_ca" || item.kind === "proxy_leaf",
  );

  async function refreshAfter(
    action: "generate" | "reissue",
    load: () => Promise<CertificateOverviewViewModel>,
  ) {
    // 生成/重签成功后统一重取 overview，页面不手工拼接证书状态。
    if (writePending) return;
    setPendingAction(action);
    try {
      const result = await load();
      toast(result.status_text, { variant: toneColor(result.ui_tone) });
      await overview.refresh();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPendingAction(undefined);
    }
  }

  async function currentLeafSans() {
    // 签发前重新读取 Rust 设置，避免使用页面打开时已经过期的 SAN。
    const latest = await callCommand(commands.settingsGet());
    settings.setData(latest);
    return latest.stored.leaf_sans;
  }

  async function exportCa() {
    // Rust 系统对话框只导出公开 Root CA；私钥没有任何 IPC 导出接口。
    if (writePending) return;
    setPendingAction("export");
    try {
      const result = await callCommand(commands.certificateExportCa());
      toast(result.message, { variant: toneColor(result.ui_tone) });
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPendingAction(undefined);
    }
  }

  async function validate() {
    if (writePending) return;
    setPendingAction("validate");
    try {
      setValidation(await callCommand(commands.certificateValidate()));
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPendingAction(undefined);
    }
  }

  async function resetCa() {
    if (writePending) return;
    setResetCaPending(true);
    try {
      const result = await callCommand(
        commands.certificateResetCa(overview.data?.revision ?? 0, true),
      );
      toast(result.status_text, { variant: toneColor(result.ui_tone) });
      await overview.refresh();
      setResetCaOpen(false);
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setResetCaPending(false);
    }
  }

  return (
    <section className="space-y-4 p-5">
      <h1 className="text-2xl font-semibold">证书管理</h1>
      <CertificateSafetyAlerts leafSansAvailable={leafSans != null} />
      {overview.error && (
        <Alert status="danger">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>证书状态读取失败</Alert.Title>
            <Alert.Description>{overview.error}</Alert.Description>
          </Alert.Content>
          <Button
            size="sm"
            variant="outline"
            onPress={() => void overview.refresh()}
          >
            重试
          </Button>
        </Alert>
      )}

      <CertificateOverviewSection
        overview={overview.data}
        localItems={localItems}
        leafSansAvailable={leafSans != null}
        writePending={writePending}
        pendingAction={pendingAction}
        onGenerate={() =>
          void refreshAfter("generate", async () =>
            callCommand(
              commands.certificateGenerateCa(await currentLeafSans()),
            ),
          )
        }
        onExport={() => void exportCa()}
        onReissue={() =>
          void refreshAfter("reissue", async () =>
            callCommand(
              commands.certificateReissueLeaf(
                overview.data!.revision,
                await currentLeafSans(),
              ),
            ),
          )
        }
        onOpenListeners={() => navigate("/listeners")}
      />
      <CertificateValidationSection
        localItems={localItems}
        isLoading={overview.isLoading}
        error={overview.error}
        validation={validation}
        writePending={writePending}
        validating={pendingAction === "validate"}
        onValidate={() => void validate()}
      />
      <CertificateResetSection
        isOpen={resetCaOpen}
        resetPending={resetCaPending}
        canReset={overview.data?.can_change ?? false}
        writePending={writePending}
        onOpenChange={(open) => {
          if (!open && resetCaPending) return;
          setResetCaOpen(open);
        }}
        onReset={() => void resetCa()}
      />
    </section>
  );
}

function CertificateSafetyAlerts({
  leafSansAvailable,
}: {
  leafSansAvailable: boolean;
}) {
  return (
    <>
      <Alert status="warning">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>本机证书材料由当前系统用户密钥保护</Alert.Title>
          <Alert.Description>
            Windows 使用 DPAPI，macOS 使用 Keychain 保护本机叶子私钥和导入的
            PKCS12 身份；密码不持久化、不记录日志，并在提交成功或关闭弹窗后清除。
          </Alert.Description>
        </Alert.Content>
      </Alert>
      <Alert status="danger">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>固定测试 Root CA 仅限受控调试环境</Alert.Title>
          <Alert.Description>
            Windows 与 macOS 使用同一张固定测试 Root CA，便于测试客户端只内置信任一次。
            该签发材料随测试工具分发，不具备生产密钥的安全边界，禁止用于生产、预生产或真实商户信任体系。
          </Alert.Description>
        </Alert.Content>
      </Alert>
      {!leafSansAvailable && (
        <Alert status="danger">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>设置快照不可用</Alert.Title>
            <Alert.Description>
              已禁用证书初始化和叶子证书重签，避免使用空 SAN
              继续执行。请先恢复 Rust 核心连接。
            </Alert.Description>
          </Alert.Content>
        </Alert>
      )}
    </>
  );
}
