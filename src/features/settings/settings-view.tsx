"use client";

/**
 * 系统设置的草稿编辑页面。
 *
 * stored 是已保存配置，effective 是当前运行实例正在使用的不可变快照，draft 是
 * 用户尚未保存的输入。Rust 负责规范化、字段校验、持久化以及是否需要重启；
 * 前端不能因输入“看起来正确”就自行判定可生效。
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
  Input,
  Label,
  NumberField,
  Switch,
  Tabs,
  TextField,
  toast,
} from "@heroui/react";
import { ArrowRotateLeft, FloppyDisk, Play } from "@gravity-ui/icons";
import type {
  ChannelSettingsDraft,
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

function mib(bytes: number) {
  // ViewModel 使用字节，页面按需求用 MiB 展示；保存时仍通过 Rust Draft 字段提交。
  return Math.round(bytes / 1024 / 1024);
}

export function SettingsView() {
  const settings = useIpcQuery<SettingsViewModel>("settings-get", () =>
    callCommand(commands.settingsGet()),
  );
  useAppEventRefresh(
    ["runtime_status_changed", "settings_changed", "snapshot_required"],
    settings.refresh,
  );
  const [draftState, setDraftState] = useState<SettingsDraft>();
  const [leafSansRaw, setLeafSansRaw] = useState<string>();
  const [validation, setValidation] =
    useState<FieldValidationViewModel>();
  const [pendingAction, setPendingAction] = useState<
    "validate" | "save" | "save_restart"
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
          (JSON.stringify(draft) !== JSON.stringify(settings.data.stored) ||
            (leafSansRaw != null &&
              leafSansRaw !== settings.data.stored.leaf_sans.join(", "))),
      ),
    [draft, leafSansRaw, settings.data],
  );
  const writePending = pendingAction != null || resetPending;

  async function validate(candidate = draft) {
    // leafSansRaw 保留用户逗号输入；Rust 负责拆分、去重、IP/DNS 合法性和规范化。
    if (!candidate || writePending) return;
    setPendingAction("validate");
    try {
      setValidation(
        await callCommand(
          commands.settingsValidate(
            candidate,
            leafSansRaw ?? candidate.leaf_sans.join(", "),
          ),
        ),
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

  async function save(restart: boolean) {
    // 保存与保存并重启是两个明确用例，运行时配置不会由前端直接修改。
    if (!draft || writePending) return;
    setPendingAction(restart ? "save_restart" : "save");
    try {
      const result = await callCommand(
        restart
          ? commands.settingsSaveAndRestart(
              draft,
              leafSansRaw ?? draft.leaf_sans.join(", "),
            )
          : commands.settingsSave(
              draft,
              leafSansRaw ?? draft.leaf_sans.join(", "),
            ),
      );
      toast(
        result.requires_restart
          ? result.restart_reason ?? "设置已保存，需要重启代理后生效。"
          : "设置已保存并生效。",
        { variant: result.requires_restart ? "warning" : "success" },
      );
      settings.setData(result);
      setDraft(result.stored);
      setLeafSansRaw(undefined);
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
      setLeafSansRaw(undefined);
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

  const effective = settings.data.effective;
  function updateChannel(
    currentDraft: SettingsDraft,
    index: number,
    update: Partial<ChannelSettingsDraft>,
  ) {
    setDraft({
      ...currentDraft,
      channels: currentDraft.channels.map((channel, channelIndex) =>
        channelIndex === index ? { ...channel, ...update } : channel,
      ),
    });
  }

  return (
    <section className="flex h-full flex-col">
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_440px] gap-4 overflow-hidden p-5 max-[1280px]:block max-[1280px]:overflow-auto">
        <div className="min-w-0 overflow-auto max-[1280px]:overflow-visible">
          <h1 className="mb-4 text-2xl font-semibold">系统设置</h1>
          <Card className="border border-[var(--telemetry-line)] shadow-sm">
            <Card.Content className="p-0">
              <Tabs defaultSelectedKey="network">
                <Tabs.ListContainer>
                  <Tabs.List aria-label="系统设置分类" className="px-3 pt-2">
                    <Tabs.Tab id="network">
                      网络与上游
                      <Tabs.Indicator />
                    </Tabs.Tab>
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
                <Tabs.Panel id="network" className="p-4">
                  <Form className="space-y-4">
                    <h2 className="text-lg font-semibold">网络与上游配置</h2>
                    <TextField isInvalid={fieldError("bind_address") != null}>
                      <Label>绑定地址</Label>
                      <Input
                        value={draft.bind_address}
                        onChange={(event) =>
                          setDraft({
                            ...draft,
                            bind_address: event.target.value,
                          })
                        }
                      />
                      {fieldError("bind_address") && (
                        <FieldError>{fieldError("bind_address")}</FieldError>
                      )}
                    </TextField>
                    <TextField isInvalid={fieldError("leaf_sans") != null}>
                      <Label>服务端证书 SAN</Label>
                      <Input
                        aria-label="服务端证书 SAN"
                        placeholder="例如：10.0.34.50, proxy.local"
                        value={leafSansRaw ?? draft.leaf_sans.join(", ")}
                        onChange={(event) => {
                          setLeafSansRaw(event.target.value);
                          setValidation(undefined);
                        }}
                      />
                      <p className="text-xs text-[var(--telemetry-muted)]">
                        填写客户端实际连接 Proxy 使用的 LAN IP 或 DNS，多个值以逗号分隔。
                      </p>
                      {fieldError("leaf_sans") && (
                        <FieldError>{fieldError("leaf_sans")}</FieldError>
                      )}
                    </TextField>
                    <div className="space-y-3">
                      {draft.channels.map((channel, index) => {
                        const portField = `channels.${channel.id}.port`;
                        const upstreamField =
                          `channels.${channel.id}.upstream_url`;
                        return (
                          <Card
                            key={channel.id}
                            className="border border-[var(--telemetry-line)] shadow-sm"
                          >
                            <Card.Content className="space-y-3 p-4">
                              <div className="flex min-w-0 items-center justify-between gap-4">
                                <div className="flex min-w-0 items-baseline gap-3">
                                  <Card.Title>{channel.display_name}</Card.Title>
                                  <Card.Description className="truncate">
                                    通道 ID：{channel.id}
                                  </Card.Description>
                                </div>
                                <Switch
                                  className="shrink-0"
                                  aria-label={`启用${channel.display_name}`}
                                  isSelected={channel.enabled}
                                  onChange={(enabled) =>
                                    updateChannel(draft, index, { enabled })
                                  }
                                >
                                  <Switch.Control>
                                    <Switch.Thumb />
                                  </Switch.Control>
                                  <Switch.Content className="sr-only">
                                    {channel.enabled ? "已启用" : "已禁用"}
                                  </Switch.Content>
                                </Switch>
                              </div>
                              <div className="grid grid-cols-[minmax(180px,0.7fr)_minmax(320px,1.8fr)] items-start gap-4 max-[680px]:grid-cols-1">
                                <NumberField
                                  isInvalid={fieldError(portField) != null}
                                  value={channel.port}
                                  minValue={1}
                                  maxValue={65535}
                                  onChange={(port) =>
                                    updateChannel(draft, index, { port })
                                  }
                                >
                                  <Label>监听端口</Label>
                                  <NumberField.Group className="w-full">
                                    <NumberField.DecrementButton />
                                    <NumberField.Input />
                                    <NumberField.IncrementButton />
                                  </NumberField.Group>
                                  {fieldError(portField) && (
                                    <FieldError>
                                      {fieldError(portField)}
                                    </FieldError>
                                  )}
                                </NumberField>
                                <TextField
                                  isInvalid={
                                    fieldError(upstreamField) != null
                                  }
                                >
                                  <Label>上游 URL</Label>
                                  <Input
                                    value={channel.upstream_url}
                                    onChange={(event) =>
                                      updateChannel(draft, index, {
                                        upstream_url: event.target.value,
                                      })
                                    }
                                  />
                                  {fieldError(upstreamField) && (
                                    <FieldError>
                                      {fieldError(upstreamField)}
                                    </FieldError>
                                  )}
                                </TextField>
                              </div>
                            </Card.Content>
                          </Card>
                        );
                      })}
                    </div>
                    <TextField>
                      <Label>TLS 版本</Label>
                      <Input value={settings.data.fixed_tls_version} readOnly />
                    </TextField>
                    <div className="grid grid-cols-2 gap-4">
                      <Switch
                        aria-label="HTTP 重定向（固定关闭）"
                        isSelected={settings.data.redirects_enabled}
                        isDisabled
                      >
                        <Switch.Control>
                          <Switch.Thumb />
                        </Switch.Control>
                        <Switch.Content>HTTP 重定向（固定关闭）</Switch.Content>
                      </Switch>
                      <Switch
                        aria-label="自动重试（固定关闭）"
                        isSelected={settings.data.retries_enabled}
                        isDisabled
                      >
                        <Switch.Control>
                          <Switch.Thumb />
                        </Switch.Control>
                        <Switch.Content>自动重试（固定关闭）</Switch.Content>
                      </Switch>
                    </div>
                    <div className="grid grid-cols-3 gap-4">
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
                    <div className="grid grid-cols-2 gap-4">
                      <Switch
                        aria-label="Host 头重写为上游主机"
                        isSelected={draft.rewrite_host}
                        onChange={(rewrite_host) =>
                          setDraft({ ...draft, rewrite_host })
                        }
                      >
                        <Switch.Control>
                          <Switch.Thumb />
                        </Switch.Control>
                        <Switch.Content>Host 头重写为上游主机</Switch.Content>
                      </Switch>
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
                          <FieldError>
                            {fieldError("max_body_bytes")}
                          </FieldError>
                        )}
                      </NumberField>
                    </div>
                    <Alert status="warning">
                      监听地址、端口、上游地址或 TLS
                      相关配置变更需要重启代理后生效。
                    </Alert>
                  </Form>
                </Tabs.Panel>
                <Tabs.Panel id="capacity" className="p-4">
                  <div className="grid grid-cols-2 gap-4">
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
                        <FieldError>
                          {fieldError("max_memory_bytes")}
                        </FieldError>
                      )}
                    </NumberField>
                  </div>
                  <Alert status="accent" className="mt-4">
                    待处理断点及其会话永不自动淘汰；容量判定使用 Rust
                    可重复计算的逻辑字节数。
                  </Alert>
                </Tabs.Panel>
                <Tabs.Panel id="data" className="space-y-4 p-4">
                  <Alert status="accent">{settings.data.payload_policy_text}</Alert>
                  <p className="text-sm">
                    Payload 仅内存保存；规则与设置持久化；敏感导出需要确认；诊断日志不记录
                    Payload、密码、私钥或 PKCS12 原始数据。
                  </p>
                </Tabs.Panel>
                <Tabs.Panel id="app" className="p-4">
                  <p className="text-sm">
                    应用启动、更新通道和诊断日志均由 Rust/Tauri
                    桌面侧管理，前端不访问文件系统或浏览器持久化。
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
              <Accordion defaultExpandedKeys={["effective", "pending", "validation"]}>
                <Accordion.Item id="effective">
                  <Accordion.Heading>
                    <Accordion.Trigger>
                      生效值（当前运行配置）
                      <Accordion.Indicator />
                    </Accordion.Trigger>
                  </Accordion.Heading>
                  <Accordion.Panel>
                    <Accordion.Body>
                      {effective ? (
                        <dl className="grid grid-cols-[120px_1fr] gap-y-2 text-sm">
                          <dt>绑定地址</dt>
                          <dd>{effective.bind_address}</dd>
                          {effective.channels.map((channel) => (
                            <div key={channel.id} className="contents">
                              <dt>{channel.display_name}</dt>
                              <dd className="break-all">
                                {channel.enabled
                                  ? `${channel.port} · ${channel.upstream_url}`
                                  : "已禁用"}
                              </dd>
                            </div>
                          ))}
                          <dt>TLS 版本</dt>
                          <dd>{settings.data.fixed_tls_version}</dd>
                        </dl>
                      ) : (
                        <p className="text-sm text-[var(--telemetry-muted)]">
                          Proxy 尚未启动，没有生效运行快照。
                        </p>
                      )}
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
                        color={settings.data.pending_changes ? "warning" : "success"}
                        variant="soft"
                      >
                        {settings.data.pending_changes
                          ? "已保存，待重启生效"
                          : "当前保存设置已生效"}
                      </Chip>
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
                  此操作只载入默认草稿，仍需保存或保存并重启。
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
              setLeafSansRaw(undefined);
            }}
          >
            放弃更改
          </Button>
          <Button
            variant="outline"
            isDisabled={!settings.data.can_write || writePending}
            onPress={() => void save(false)}
          >
            <FloppyDisk className="size-4" />
            {pendingAction === "save" ? "正在保存…" : "保存设置"}
          </Button>
          <Button
            variant="primary"
            isDisabled={!settings.data.can_write || writePending}
            onPress={() => void save(true)}
          >
            <Play className="size-4" />
            {pendingAction === "save_restart"
              ? "正在保存并重启…"
              : "保存并重启代理"}
          </Button>
        </div>
      </footer>
    </section>
  );
}
