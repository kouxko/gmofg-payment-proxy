"use client";

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
      setPassword("");
      setPkcs12Open(false);
    } catch (reason) {
      setPkcs12Error(errorMessage(reason));
    } finally {
      setPkcs12Pending(false);
    }
  }

  async function currentLeafSans() {
    const latest = await callCommand(commands.settingsGet());
    settings.setData(latest);
    return latest.stored.leaf_sans;
  }

  async function exportCa() {
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
          <Alert.Title>统一测试 Root CA 仅限隔离测试环境</Alert.Title>
          <Alert.Description>
            当前简化测试模式随安装包提供统一签发能力，凭据可能被提取；不得用于生产、
            预生产或真实商户信任体系。Root CA 首次集成与轮换由受控的测试 Payment
            构建/发布流程完成。
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

      <div className="grid grid-cols-2 items-start gap-4 max-[1180px]:grid-cols-1">
        <Card>
          <Card.Header>
            <Card.Title>A. 统一测试 CA 与 App → Proxy 服务端身份</Card.Title>
          </Card.Header>
          <Card.Content className="space-y-4">
            {(overview.data?.items ?? []).map((item) => (
              <div
                key={item.kind}
                className="space-y-2 border-b border-[var(--telemetry-line)] pb-4 last:border-0"
              >
                <div className="flex items-center gap-2 font-semibold">
                  {item.usage}
                  <Chip
                    size="sm"
                    color={toneColor(item.ui_tone)}
                    variant="soft"
                  >
                    {item.status_text}
                  </Chip>
                </div>
                <dl className="grid grid-cols-[150px_1fr] gap-y-2 text-sm">
                  <dt>主题</dt>
                  <dd>{item.subject}</dd>
                  <dt>SAN</dt>
                  <dd>{item.sans.join("、") || "—"}</dd>
                  <dt>有效期</dt>
                  <dd>
                    {formatTimestamp(item.valid_from)} ～{" "}
                    {formatTimestamp(item.valid_until)}
                  </dd>
                  <dt>SHA-256 指纹</dt>
                  <dd className="break-all font-mono text-xs">
                    {item.sha256_fingerprint}
                  </dd>
                </dl>
              </div>
            ))}
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
                    : "初始化本机测试证书"}
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
                  : "导出测试 Payment 编译用 Root CA"}
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
            <Card.Title>B. Proxy → GMO-FG Server 客户端身份</Card.Title>
          </Card.Header>
          <Card.Content className="space-y-4">
            <p className="text-sm text-[var(--telemetry-muted)]">
              导入 Launcher 原有客户端 P12 作为上游客户端身份。Payment 原始
              server.crt 已内置并默认作为 GMO-FG Server 信任锚；环境证书变化时
              可以选择文件替换。上游信任与下游 Proxy 测试 CA 无关。
            </p>
            <div className="flex flex-wrap gap-3">
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
                        <Modal.Heading>导入共享 PKCS12</Modal.Heading>
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
                    初始化本机测试证书或导入上游材料后，此处显示证书状态与校验详情。
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
              "Payment (App) → Proxy（服务端证书）：测试版 Payment 在构建时已内置统一测试 Root CA。",
              "Proxy（服务端）→ Payment（客户端证书）：Proxy 验证终端客户端证书。",
              "GMO-FG Server → Proxy（共享客户端证书）：Server 验证 Proxy 身份。",
              "Proxy（客户端）→ GMO-FG Server：Proxy 使用上游 CA 验证 Server。",
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
            将使用同一统一测试 Root CA 重新生成本机私钥和叶子证书；Payment
            无需重新导入证书。仅 Proxy 已停止时可执行。
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
                  本机服务端私钥和叶子证书将被替换；统一测试 Root CA
                  保持不变，Payment 无需重新编译或导入。
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
