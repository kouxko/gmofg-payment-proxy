"use client";

/**
 * 本机调试 Root CA 与代理服务端证书管理页面。
 *
 * 所有证书读取、生成、验证、文件对话框和密钥保护都由 Rust 执行。前端绝不接触
 * 私钥字节。某条监听启用固定 Server 后使用的 Server CA 与可选 mTLS 客户端身份，统一在
 * “代理入口”页面按监听导入、选择并测试，避免全局证书误用于其他 Server。
 */

import { useState } from "react";
import {
  Alert,
  AlertDialog,
  Button,
  Card,
  Chip,
  Spinner,
  Table,
  toast,
} from "@heroui/react";
import {
  ArrowDownToLine,
  Shield,
  TrashBin,
} from "@gravity-ui/icons";
import type {
  CertificateOverviewViewModel,
  FieldValidationViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { formatTimestamp, toneColor } from "@/lib/format";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";

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
  const [pendingAction, setPendingAction] = useState<
    "generate" | "reissue" | "export" | "validate"
  >();
  const [resetCaOpen, setResetCaOpen] = useState(false);
  const [resetCaPending, setResetCaPending] = useState(false);
  const [validation, setValidation] =
    useState<FieldValidationViewModel>();
  const leafSans = settings.data?.stored.leaf_sans;
  const writePending = pendingAction != null || resetCaPending;
  const localItems = (overview.data?.items ?? []).filter(
    (item) => item.kind === "local_root_ca" || item.kind === "proxy_leaf",
  );

  async function refreshAfter(
    action: Exclude<
      NonNullable<typeof pendingAction>,
      "export" | "validate"
    >,
    load: () => Promise<CertificateOverviewViewModel>,
  ) {
    // 生成/重签/导入成功后统一重取 overview，页面不手工拼接证书状态。
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
      setValidation(
        await callCommand(commands.certificateValidate()),
      );
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
        commands.certificateResetCa(
          overview.data?.revision ?? 0,
          true,
        ),
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
      {!leafSans && (
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

      <div className="space-y-4">
        <Card>
          <Card.Header>
            <Card.Title>A. 固定测试 Root CA 与客户端 → Proxy 服务端身份</Card.Title>
          </Card.Header>
          <Card.Content className="space-y-4">
            <div
              data-testid="certificate-overview-grid"
              className="grid grid-cols-2 gap-x-6 gap-y-4 max-[960px]:grid-cols-1"
            >
              {localItems.map((item) => (
                <div
                  key={item.kind}
                  className="min-w-0 space-y-2 border-b border-[var(--telemetry-line)] pb-4"
                >
                  <div className="flex flex-wrap items-center gap-2 font-semibold">
                    <span className="min-w-0 break-words">{item.usage}</span>
                    <Chip
                      size="sm"
                      color={toneColor(item.ui_tone)}
                      variant="soft"
                    >
                      {item.status_text}
                    </Chip>
                  </div>
                  <dl className="grid min-w-0 grid-cols-[112px_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm max-[560px]:grid-cols-1 max-[560px]:gap-y-1">
                    <dt>主题</dt>
                    <dd className="min-w-0 break-words">{item.subject}</dd>
                    <dt>SAN</dt>
                    <dd className="min-w-0 break-words">
                      {item.sans.join("、") || "—"}
                    </dd>
                    <dt>有效期</dt>
                    <dd className="min-w-0 break-words">
                      {formatTimestamp(item.valid_from)} ～{" "}
                      {formatTimestamp(item.valid_until)}
                    </dd>
                    <dt>SHA-256 指纹</dt>
                    <dd className="min-w-0 break-all font-mono text-xs">
                      {item.sha256_fingerprint}
                    </dd>
                  </dl>
                </div>
              ))}
            </div>
            <div className="flex flex-wrap gap-3">
              {overview.data?.can_initialize && (
                <Button
                  variant="primary"
                  isDisabled={
                    !overview.data?.can_change || !leafSans || writePending
                  }
                  onPress={() =>
                    void refreshAfter(
                      "generate",
                      async () =>
                        callCommand(
                          commands.certificateGenerateCa(
                            await currentLeafSans(),
                          ),
                        ),
                    )
                  }
                >
                  {pendingAction === "generate"
                    ? "正在生成…"
                    : "初始化本机证书"}
                </Button>
              )}
              <Button
                variant="outline"
                isDisabled={writePending}
                onPress={() => void exportCa()}
              >
                <ArrowDownToLine className="size-4" />
                {pendingAction === "export"
                  ? "正在导出…"
                  : "导出公开 Root CA"}
              </Button>
              <Button
                variant="outline"
                isDisabled={
                  !overview.data?.can_change || !leafSans || writePending
                }
                onPress={() =>
                  void refreshAfter(
                    "reissue",
                    async () =>
                      callCommand(
                        commands.certificateReissueLeaf(
                          overview.data!.revision,
                          await currentLeafSans(),
                        ),
                      ),
                  )
                }
              >
                {pendingAction === "reissue"
                  ? "正在重签…"
                  : "重新签发服务端证书"}
              </Button>
            </div>
          </Card.Content>
        </Card>

        <Alert status="accent">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>Server TLS/mTLS 按代理监听配置</Alert.Title>
            <Alert.Description>
              每条启用固定 Server 的监听可以分别使用不同的 Server CA、主机名校验策略和可选
              PKCS12 客户端身份。请在对应监听中导入并执行真实握手测试。
            </Alert.Description>
          </Alert.Content>
          <Button variant="outline" onPress={() => navigate("/listeners")}>去配置代理入口</Button>
        </Alert>
      </div>

      <div className="grid grid-cols-[1fr_1.2fr] items-start gap-4 max-[1180px]:grid-cols-1">
        <Card>
          <Card.Header>
            <Card.Title>证书检查结果</Card.Title>
            <Button
              className="ml-auto"
              size="sm"
              variant="outline"
              isDisabled={writePending}
              onPress={() => void validate()}
            >
              {pendingAction === "validate" ? "正在检查…" : "重新检查"}
            </Button>
          </Card.Header>
          <Card.Content>
            {overview.isLoading ? (
              <div className="grid min-h-32 place-items-center">
                <Spinner aria-label="正在读取证书检查结果" />
              </div>
            ) : overview.error ? (
              <Alert status="danger">
                <Alert.Content>
                  <Alert.Title>证书检查结果暂不可用</Alert.Title>
                  <Alert.Description>{overview.error}</Alert.Description>
                </Alert.Content>
              </Alert>
            ) : localItems.length === 0 ? (
              <Alert status="default">
                <Alert.Content>
                  <Alert.Title>暂无证书检查结果</Alert.Title>
                  <Alert.Description>
                    初始化本机证书后，此处显示 Root CA 与服务端证书状态。
                  </Alert.Description>
                </Alert.Content>
              </Alert>
            ) : (
              <Table>
                <Table.ScrollContainer>
                  <Table.Content aria-label="证书检查结果">
                    <Table.Header>
                      <Table.Column isRowHeader>检查项</Table.Column>
                      <Table.Column>状态</Table.Column>
                      <Table.Column>详情</Table.Column>
                    </Table.Header>
                    <Table.Body>
                      {localItems.map((item) => (
                        <Table.Row key={item.kind} id={item.kind}>
                          <Table.Cell>{item.usage}</Table.Cell>
                          <Table.Cell>{item.status_text}</Table.Cell>
                          <Table.Cell>{item.subject}</Table.Cell>
                        </Table.Row>
                      ))}
                    </Table.Body>
                  </Table.Content>
                </Table.ScrollContainer>
              </Table>
            )}
            {validation && (
              <Alert
                status={validation.valid ? "success" : "danger"}
                className="mt-3"
              >
                {validation.valid
                  ? validation.warnings.join("；") || "全部证书检查通过。"
                  : Object.values(validation.field_errors).flat().join("；")}
              </Alert>
            )}
          </Card.Content>
        </Card>
        <Card>
          <Card.Header>
            <Card.Title>证书信任关系说明</Card.Title>
          </Card.Header>
          <Card.Content className="space-y-3 text-sm">
            {[
              "客户端 → Proxy（服务端证书）：客户端信任本机导出的公开 Root CA，并校验叶子证书 SAN。",
              "Proxy → 客户端（客户端证书）：仅当入口启用可选或必须客户端认证时，Proxy 才校验客户端证书。",
              "上游服务器 → Proxy（客户端身份）：仅当上游要求 mTLS 时，Proxy 才提交所选 PKCS12 身份。",
              "Proxy → 上游服务器：Proxy 按入口配置的 CA 与主机名策略校验上游服务器。",
            ].map((text, index) => (
              <div key={text} className="flex gap-3">
                <Chip size="sm" variant="soft">
                  {index + 1}
                </Chip>
                <span>{text}</span>
              </div>
            ))}
          </Card.Content>
        </Card>
      </div>

      <Alert status="danger">
        <Alert.Indicator>
          <Shield className="size-5" />
        </Alert.Indicator>
        <Alert.Content>
          <Alert.Title>恢复固定测试证书并重签叶子证书</Alert.Title>
          <Alert.Description>
            将恢复内置的固定测试 Root CA，并按当前 SAN 重新生成本机叶子证书。
            已信任该固定 Root CA 的客户端无需重新导入；仅所有代理入口均已停止时可执行。
          </Alert.Description>
        </Alert.Content>
        <AlertDialog
          isOpen={resetCaOpen}
          onOpenChange={(open) => {
            if (!open && resetCaPending) return;
            setResetCaOpen(open);
          }}
        >
          <Button
            variant="danger"
            isDisabled={!overview.data?.can_change || writePending}
          >
            <TrashBin className="size-4" />
            恢复固定测试证书
          </Button>
          <AlertDialog.Backdrop>
            <AlertDialog.Container>
              <AlertDialog.Dialog>
                <AlertDialog.Header>
                  <AlertDialog.Heading>确认恢复固定测试证书？</AlertDialog.Heading>
                </AlertDialog.Header>
                <AlertDialog.Body>
                  将恢复应用内置的固定测试 Root CA，并重新生成本机服务端叶子证书。
                  Root CA 指纹保持不变，但当前连接会被停止，因此仅可在所有代理入口停止后执行。
                </AlertDialog.Body>
                <AlertDialog.Footer>
                  <Button
                    slot="close"
                    variant="outline"
                    isDisabled={resetCaPending}
                  >
                    取消
                  </Button>
                  <Button
                    variant="danger"
                    isDisabled={resetCaPending}
                    onPress={() => void resetCa()}
                  >
                    {resetCaPending ? "正在重置…" : "确认重置"}
                  </Button>
                </AlertDialog.Footer>
              </AlertDialog.Dialog>
            </AlertDialog.Container>
          </AlertDialog.Backdrop>
        </AlertDialog>
      </Alert>
    </section>
  );
}
