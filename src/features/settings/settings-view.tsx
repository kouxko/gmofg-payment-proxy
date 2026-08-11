"use client";

/**
 * 系统设置的草稿编辑页面。
 *
 * 本页只管理与具体代理入口无关的全局超时、容量和应用策略。监听地址、端口、
 * 上游、TLS 与入口启停统一由“入口配置”负责，避免同一网络参数出现两处入口。
 * Rust 负责规范化、字段校验和持久化，前端只维护当前表单草稿。
 */

import { useMemo, useState } from "react";
import {
  Accordion,
  Alert,
  AlertDialog,
  Button,
  Card,
  Chip,
  FieldError,
  Form,
  Label,
  NumberField,
  Switch,
  Tabs,
  toast,
} from "@heroui/react";
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
import { ThemeSettings } from "./settings-content";

function mib(bytes: number) {
  // ViewModel 使用字节，页面按需求用 MiB 展示；保存时仍通过 Rust Draft 字段提交。
  return Math.round(bytes / 1024 / 1024);
}

export function SettingsView() {
  const settings = useIpcQuery<SettingsViewModel>("settings-get", () =>
    callCommand(commands.settingsGet()),
  );
  useAppEventRefresh(
    ["settings_changed", "snapshot_required"],
    settings.refresh,
  );
  const [draftState, setDraftState] = useState<SettingsDraft>();
  const [validation, setValidation] =
    useState<FieldValidationViewModel>();
  const [pendingAction, setPendingAction] = useState<
    "validate" | "save"
  >();
  const [resetDialogOpen, setResetDialogOpen] = useState(false);
  const [resetPending, setResetPending] = useState(false);
  const fieldError = (field: string) =>
    validation?.field_errors[field]?.join("；");
  function setDraft(next: SettingsDraft | undefined) {
    // 用户继续编辑后，旧校验结果不再可信，必须清除并重新请求 Rust 校验。
    setDraftState(next);
    setValidation(undefined);
  }
  const draft = draftState ?? settings.data?.stored;
  const draftDirty = useMemo(
    // 只用于提示“有未保存改动”，不承担业务字段校验。
    () =>
      Boolean(
        draft &&
          settings.data &&
          JSON.stringify(draft) !== JSON.stringify(settings.data.stored),
      ),
    [draft, settings.data],
  );
  const writePending = pendingAction != null || resetPending;

  async function validate(candidate = draft) {
    // 通用产品不再从系统设置编辑证书 SAN；完整 Draft 直接交由 Rust 校验。
    if (!candidate || writePending) return;
    setPendingAction("validate");
    try {
      setValidation(
        await callCommand(commands.settingsValidate(candidate)),
      );
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

  async function save() {
    // 系统设置只保存全局容量与应用策略；代理入口在“入口配置”中独立启停。
    if (!draft || writePending) return;
    setPendingAction("save");
    try {
      const result = await callCommand(commands.settingsSave(draft));
      toast(
        result.requires_restart
          ? result.restart_reason ?? "设置已保存，需要重新打开应用后生效。"
          : "设置已保存并生效。",
        { variant: result.requires_restart ? "warning" : "success" },
      );
      settings.setData(result);
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
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_440px] gap-4 overflow-hidden p-5 max-[1280px]:block max-[1280px]:overflow-auto">
        <div className="min-w-0 overflow-auto max-[1280px]:overflow-visible">
          <h1 className="mb-4 text-2xl font-semibold">系统设置</h1>
          <Card className="border border-[var(--telemetry-line)] shadow-sm">
            <Card.Content className="p-0">
              <Tabs defaultSelectedKey="capacity">
                <Tabs.ListContainer>
                  <Tabs.List aria-label="系统设置分类" className="px-3 pt-2">
                    <Tabs.Tab id="capacity">
                      超时与容量
                      <Tabs.Indicator />
                    </Tabs.Tab>
                    <Tabs.Tab id="data">
                      数据与导出
                      <Tabs.Indicator />
                    </Tabs.Tab>
                    <Tabs.Tab id="app">
                      应用
                      <Tabs.Indicator />
                    </Tabs.Tab>
                  </Tabs.List>
                </Tabs.ListContainer>
                <Tabs.Panel id="capacity" className="p-4">
                  <Form className="space-y-5">
                    <Alert status="accent">
                      代理入口的监听地址、端口、上游和 TLS 请统一到“入口配置”中管理。
                    </Alert>
                    <div className="grid grid-cols-3 gap-4 max-[760px]:grid-cols-1">
                      {[
                        ["连接超时（秒）", "connect_timeout_seconds"],
                        ["写入超时（秒）", "write_timeout_seconds"],
                        ["读取超时（秒）", "read_timeout_seconds"],
                      ].map(([label, key]) => (
                        <NumberField
                          key={key}
                          isInvalid={fieldError(key) != null}
                          value={draft[key as keyof SettingsDraft] as number}
                          minValue={1}
                          onChange={(value) =>
                            setDraft({ ...draft, [key]: value })
                          }
                        >
                          <Label>{label}</Label>
                          <NumberField.Group className="w-full">
                            <NumberField.DecrementButton />
                            <NumberField.Input />
                            <NumberField.IncrementButton />
                          </NumberField.Group>
                          {fieldError(key) && (
                            <FieldError>{fieldError(key)}</FieldError>
                          )}
                        </NumberField>
                      ))}
                    </div>
                    <div className="grid grid-cols-2 gap-4 max-[760px]:grid-cols-1">
                      <NumberField
                        isInvalid={fieldError("max_sessions") != null}
                        value={draft.max_sessions}
                        minValue={1}
                        onChange={(max_sessions) =>
                          setDraft({ ...draft, max_sessions })
                        }
                      >
                        <Label>最大会话数</Label>
                        <NumberField.Group className="w-full">
                          <NumberField.DecrementButton />
                          <NumberField.Input />
                          <NumberField.IncrementButton />
                        </NumberField.Group>
                        {fieldError("max_sessions") && (
                          <FieldError>{fieldError("max_sessions")}</FieldError>
                        )}
                      </NumberField>
                      <NumberField
                        isInvalid={fieldError("max_memory_bytes") != null}
                        value={mib(draft.max_memory_bytes)}
                        minValue={1}
                        onChange={(value) =>
                          setDraft({
                            ...draft,
                            max_memory_bytes: value * 1024 * 1024,
                          })
                        }
                      >
                        <Label>最大内存 MiB</Label>
                        <NumberField.Group className="w-full">
                          <NumberField.DecrementButton />
                          <NumberField.Input />
                          <NumberField.IncrementButton />
                        </NumberField.Group>
                        {fieldError("max_memory_bytes") && (
                          <FieldError>{fieldError("max_memory_bytes")}</FieldError>
                        )}
                      </NumberField>
                      <NumberField
                        isInvalid={fieldError("max_body_bytes") != null}
                        value={mib(draft.max_body_bytes)}
                        minValue={1}
                        onChange={(value) =>
                          setDraft({
                            ...draft,
                            max_body_bytes: value * 1024 * 1024,
                          })
                        }
                      >
                        <Label>请求体大小限制 MiB</Label>
                        <NumberField.Group className="w-full">
                          <NumberField.DecrementButton />
                          <NumberField.Input />
                          <NumberField.IncrementButton />
                        </NumberField.Group>
                        {fieldError("max_body_bytes") && (
                          <FieldError>{fieldError("max_body_bytes")}</FieldError>
                        )}
                      </NumberField>
                      <Switch
                        aria-label="Host 头重写为目标主机"
                        isSelected={draft.rewrite_host}
                        onChange={(rewrite_host) =>
                          setDraft({ ...draft, rewrite_host })
                        }
                      >
                        <Switch.Content>
                          <Switch.Control>
                            <Switch.Thumb />
                          </Switch.Control>
                          <span>Host 头重写为目标主机</span>
                        </Switch.Content>
                      </Switch>
                    </div>
                    <Alert status="accent">
                      待处理断点及其会话永不自动淘汰；容量判定使用 Rust 可重复计算的逻辑字节数。
                    </Alert>
                  </Form>
                </Tabs.Panel>
                <Tabs.Panel id="data" className="space-y-4 p-4">
                  <Alert status="accent">{settings.data.payload_policy_text}</Alert>
                  <p className="text-sm">
                    Payload 仅内存保存；规则与设置持久化；敏感导出需要确认；诊断日志不记录
                    Payload、密码、私钥或 PKCS12 原始数据。
                  </p>
                </Tabs.Panel>
                <Tabs.Panel id="app" className="space-y-4 p-4">
                  <Alert status="accent">
                    系统设置只管理全局行为；入口配置、证书和规则分别在对应页面管理。
                  </Alert>
                  <ThemeSettings />
                  <p className="text-sm">
                    应用启动和诊断日志由 Rust/Tauri 桌面侧管理；外观主题仅保存在本机浏览器存储中。
                  </p>
                </Tabs.Panel>
              </Tabs>
            </Card.Content>
          </Card>
        </div>

        <aside className="overflow-auto max-[1280px]:mt-4 max-[1280px]:overflow-visible">
          <Card className="border border-[var(--telemetry-line)] shadow-sm">
            <Card.Header>
              <Card.Title>配置摘要与校验</Card.Title>
            </Card.Header>
            <Card.Content className="space-y-4">
              <Accordion defaultExpandedKeys={["stored", "pending", "validation"]}>
                <Accordion.Item id="stored">
                  <Accordion.Heading>
                    <Accordion.Trigger>
                      已保存的全局设置
                      <Accordion.Indicator />
                    </Accordion.Trigger>
                  </Accordion.Heading>
                  <Accordion.Panel>
                    <Accordion.Body>
                      <dl className="grid grid-cols-[120px_1fr] gap-y-2 text-sm">
                        <dt>连接超时</dt>
                        <dd>{settings.data.stored.connect_timeout_seconds} 秒</dd>
                        <dt>写入超时</dt>
                        <dd>{settings.data.stored.write_timeout_seconds} 秒</dd>
                        <dt>读取超时</dt>
                        <dd>{settings.data.stored.read_timeout_seconds} 秒</dd>
                        <dt>最大会话数</dt>
                        <dd>{settings.data.stored.max_sessions}</dd>
                        <dt>最大内存</dt>
                        <dd>{mib(settings.data.stored.max_memory_bytes)} MiB</dd>
                      </dl>
                      <p className="mt-3 text-xs text-[var(--telemetry-muted)]">
                        监听端口、请求去向和入口启停不属于系统设置。
                      </p>
                    </Accordion.Body>
                  </Accordion.Panel>
                </Accordion.Item>
                <Accordion.Item id="pending">
                  <Accordion.Heading>
                    <Accordion.Trigger>
                      保存与生效状态
                      <Accordion.Indicator />
                    </Accordion.Trigger>
                  </Accordion.Heading>
                  <Accordion.Panel>
                    <Accordion.Body className="flex flex-wrap gap-2">
                      <Chip
                        color={draftDirty ? "warning" : "success"}
                        variant="soft"
                      >
                        {draftDirty
                          ? "存在未保存草稿"
                          : "草稿与已保存设置一致"}
                      </Chip>
                    </Accordion.Body>
                  </Accordion.Panel>
                </Accordion.Item>
                <Accordion.Item id="validation">
                  <Accordion.Heading>
                    <Accordion.Trigger>
                      校验结果
                      <Accordion.Indicator />
                    </Accordion.Trigger>
                  </Accordion.Heading>
                  <Accordion.Panel>
                    <Accordion.Body>
                      {!validation ? (
                        <Button
                          variant="outline"
                          isDisabled={writePending}
                          onPress={() => void validate()}
                        >
                          {pendingAction === "validate"
                            ? "正在校验…"
                            : "运行 Rust 校验"}
                        </Button>
                      ) : (
                        <Alert
                          status={validation.valid ? "success" : "danger"}
                        >
                          {validation.valid
                            ? validation.warnings.join("；") || "全部检查通过。"
                            : Object.values(validation.field_errors)
                                .flat()
                                .join("；")}
                        </Alert>
                      )}
                    </Accordion.Body>
                  </Accordion.Panel>
                </Accordion.Item>
              </Accordion>
            </Card.Content>
          </Card>
        </aside>
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
        <div className="ml-auto flex gap-3">
          <Button
            variant="outline"
            isDisabled={writePending}
            onPress={() => {
              setDraft(settings.data?.stored);
            }}
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
