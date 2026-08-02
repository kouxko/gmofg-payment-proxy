"use client";

/**
 * 两段 mTLS 证书材料管理页面。
 *
 * 所有证书读取、生成、验证、文件对话框和密钥保护都由 Rust 执行。前端绝不接触
 * 私钥字节，也不保存 PKCS12 密码；密码仅存在于当前弹窗状态，成功或关闭即清除。
 */

import { useState } from "react";
import {
  Alert,
  AlertDialog,
  Button,
  Card,
  Chip,
  Input,
  Label,
  Modal,
  Spinner,
  Table,
  TextField,
  toast,
} from "@heroui/react";
import {
  ArrowDownToLine,
  FileArrowUp,
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

export function CertificatesView() {
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
  const [password, setPassword] = useState("");
  const [pkcs12Open, setPkcs12Open] = useState(false);
  const [pkcs12Pending, setPkcs12Pending] = useState(false);
  const [pkcs12Error, setPkcs12Error] = useState<string>();
  const [pendingAction, setPendingAction] = useState<
    "generate" | "reissue" | "import_upstream" | "export" | "validate"
  >();
  const [resetCaOpen, setResetCaOpen] = useState(false);
  const [resetCaPending, setResetCaPending] = useState(false);
  const [validation, setValidation] =
    useState<FieldValidationViewModel>();
  const leafSans = settings.data?.stored.leaf_sans;
  const writePending =
    pendingAction != null || pkcs12Pending || resetCaPending;

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

  async function importPkcs12() {
    if (writePending) return;
    setPkcs12Pending(true);
    setPkcs12Error(undefined);
    try {
      const result = await callCommand(
        commands.certificateImportPkcs12(password),
      );
      toast(result.status_text, { variant: toneColor(result.ui_tone) });
      await overview.refresh();
      // 密码没有持久化需求，导入成功后立即从 React state 清除。
      setPassword("");
      setPkcs12Open(false);
    } catch (reason) {
      setPkcs12Error(errorMessage(reason));
    } finally {
      setPkcs12Pending(false);
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
          <Alert.Title>本机 Root CA 仅限受控调试环境</Alert.Title>
          <Alert.Description>
            每个 Intercept Proxy 安装实例都会生成独立 Root CA，私钥仅保存在当前系统用户的受保护存储中且不可导出。
            请勿把该 CA 用于生产、预生产或真实商户信任体系。
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
            <Card.Title>A. 本机 Root CA 与客户端 → Proxy 服务端身份</Card.Title>
          </Card.Header>
          <Card.Content className="space-y-4">
            <div
              data-testid="certificate-overview-grid"
              className="grid grid-cols-2 gap-x-6 gap-y-4 max-[960px]:grid-cols-1"
            >
              {(overview.data?.items ?? []).map((item) => (
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

        <Card>
          <Card.Header>
            <Card.Title>B. 上游 TLS 信任与可选客户端身份</Card.Title>
          </Card.Header>
          <Card.Content
            data-testid="certificate-upstream-actions"
            className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-4 max-[860px]:grid-cols-1"
          >
            <p className="text-sm text-[var(--telemetry-muted)]">
              上游要求 mTLS 时才需要导入 PKCS12 客户端身份；普通 TLS 可以不配置。
              上游 CA 可按入口通过安全引用选择，与下游使用的本机 Root CA 相互独立。
            </p>
            <div className="flex flex-wrap justify-end gap-3 max-[860px]:justify-start">
              <Modal
                isOpen={pkcs12Open}
                onOpenChange={(open) => {
                  if (!open && pkcs12Pending) return;
                  setPkcs12Open(open);
                  if (!open) {
                    setPassword("");
                    setPkcs12Error(undefined);
                  }
                }}
              >
                <Button
                  variant="outline"
                  isDisabled={!overview.data?.can_change || writePending}
                >
                  <FileArrowUp className="size-4" />
                  导入 / 替换 PKCS12
                </Button>
                <Modal.Backdrop isDismissable={!pkcs12Pending}>
                  <Modal.Container size="sm">
                    <Modal.Dialog>
                      <Modal.Header>
                        <Modal.Heading>导入上游 PKCS12 客户端身份</Modal.Heading>
                      </Modal.Header>
                      <Modal.Body>
                        <TextField>
                          <Label>PKCS12 密码</Label>
                          <Input
                            type="password"
                            value={password}
                            onChange={(event) => setPassword(event.target.value)}
                          />
                        </TextField>
                        <p className="mt-2 text-sm text-[var(--telemetry-muted)]">
                          文件选择和原始字节读取由 Rust 原生侧完成。
                        </p>
                        {pkcs12Error && (
                          <Alert status="danger" className="mt-3">
                            <Alert.Indicator />
                            <Alert.Content>
                              <Alert.Title>PKCS12 导入失败</Alert.Title>
                              <Alert.Description>
                                {pkcs12Error}
                              </Alert.Description>
                            </Alert.Content>
                          </Alert>
                        )}
                      </Modal.Body>
                      <Modal.Footer>
                        <Button
                          slot="close"
                          variant="outline"
                          isDisabled={pkcs12Pending}
                        >
                          取消
                        </Button>
                        <Button
                          variant="primary"
                          isDisabled={pkcs12Pending}
                          onPress={() => void importPkcs12()}
                        >
                          {pkcs12Pending ? "正在导入…" : "选择文件并导入"}
                        </Button>
                      </Modal.Footer>
                    </Modal.Dialog>
                  </Modal.Container>
                </Modal.Backdrop>
              </Modal>
              <Button
                variant="outline"
                isDisabled={!overview.data?.can_change || writePending}
                onPress={() =>
                  void refreshAfter(
                    "import_upstream",
                    () => callCommand(commands.certificateImportUpstreamCa()),
                  )
                }
              >
                <FileArrowUp className="size-4" />
                {pendingAction === "import_upstream"
                  ? "正在导入…"
                  : "选择性替换上游 CA"}
              </Button>
            </div>
          </Card.Content>
        </Card>
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
            ) : (overview.data?.items.length ?? 0) === 0 ? (
              <Alert status="default">
                <Alert.Content>
                  <Alert.Title>暂无证书检查结果</Alert.Title>
                  <Alert.Description>
                    初始化本机证书或导入上游材料后，此处显示证书状态与校验详情。
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
                      {(overview.data?.items ?? []).map((item) => (
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
          <Alert.Title>重新初始化本机服务端证书（危险操作）</Alert.Title>
          <Alert.Description>
            将替换本机 Root CA、Root 私钥、服务端私钥和叶子证书。此前信任旧 Root CA
            的客户端必须重新导入新公开证书；仅所有代理入口均已停止时可执行。
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
            重新初始化本机证书
          </Button>
          <AlertDialog.Backdrop>
            <AlertDialog.Container>
              <AlertDialog.Dialog>
                <AlertDialog.Header>
                  <AlertDialog.Heading>确认重新初始化本机证书？</AlertDialog.Heading>
                </AlertDialog.Header>
                <AlertDialog.Body>
                  本机 Root CA、Root 私钥、服务端私钥和叶子证书都会被替换。
                  已信任旧 Root CA 的客户端将无法继续连接，必须重新导入新公开证书。
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
