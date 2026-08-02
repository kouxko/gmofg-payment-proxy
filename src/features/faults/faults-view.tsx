"use client";

/**
 * 故障模拟模板页面。
 *
 * 模板只是创建普通拦截规则的快捷入口，不存在第二套故障引擎。Rust 返回模板、
 * 默认参数和字段错误，并把“立即启用/保存为规则”统一转换为规则；前端只维护
 * 当前表单覆盖值和危险操作确认。
 */

import { useMemo, useRef, useState } from "react";
import {
  Alert,
  AlertDialog,
  Button,
  Card,
  Chip,
  FieldError,
  Input,
  Label,
  ListBox,
  NumberField,
  Select,
  Switch,
  Table,
  TextArea,
  TextField,
  toast,
} from "@heroui/react";
import type {
  ActiveFaultViewModel,
  ChannelId,
  FaultConfigurationDraft,
  FaultParameterValue,
  FaultTemplateViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import {
  appErrorViewModel,
  callCommand,
  errorMessage,
} from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { toneColor } from "@/lib/format";
import {
  useAppEventRefresh,
  useBootstrap,
} from "@/features/shell/bootstrap-context";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";

export function FaultsView() {
  const { bootstrap } = useBootstrap();
  const channelCatalog = bootstrap?.channel_catalog ?? [];
  const { navigate } = useWorkspaceNavigation();
  const templates =
    useIpcQuery<FaultTemplateViewModel[]>("fault-template-list", () =>
      callCommand(commands.faultTemplateList()),
    );
  const active = useIpcQuery<ActiveFaultViewModel[]>("fault-active-list", () =>
    callCommand(commands.faultActiveList()),
  );
  useAppEventRefresh(["rule_hit", "snapshot_required"], active.refresh);
  const [selectedId, setSelectedId] = useState<string>();
  const [configurePending, setConfigurePending] = useState<
    "enable" | "save"
  >();
  const [fieldErrors, setFieldErrors] = useState<Record<string, string[]>>({});
  const [stopDialogRuleId, setStopDialogRuleId] = useState<string>();
  const [stoppingRuleId, setStoppingRuleId] = useState<string>();
  const configurationPanelRef = useRef<HTMLElement>(null);
  const [terminal, setTerminal] = useState("");
  const [target, setTarget] = useState("");
  const [nthHit, setNthHit] = useState<number>();
  const [priority, setPriority] = useState<number>();
  const [oneShot, setOneShot] = useState<boolean>();
  const [channel, setChannel] = useState<ChannelId>();
  const [parameterOverrides, setParameterOverrides] = useState<
    Record<string, Record<string, FaultParameterValue>>
  >({});

  const effectiveSelectedId =
    // 用户尚未选择时默认使用第一项，点击页面即可直接配置第一个模板。
    selectedId ?? templates.data?.[0]?.template_id;
  const writePending = configurePending != null || stoppingRuleId != null;
  const fieldError = (field: string) => fieldErrors[field]?.join("；");

  const selected = templates.data?.find(
    (template) => template.template_id === effectiveSelectedId,
  );
  const parameters = useMemo(
    () =>
      selected
        ? (parameterOverrides[selected.template_id] ??
          selected.default_parameters)
        : {},
    [parameterOverrides, selected],
  );
  const effectiveNthHit = nthHit ?? selected?.default_nth_hit;
  const effectivePriority = priority ?? selected?.default_priority;
  const effectiveOneShot = oneShot ?? selected?.default_one_shot;
  const effectiveChannel = channel ?? selected?.default_channel;

  const draft = useMemo<FaultConfigurationDraft | undefined>(() => {
    // 只在 Rust 给出的必需默认值齐全时组装提交 DTO，不在此判断动作语义。
    if (
      !selected ||
      effectiveChannel == null ||
      effectiveNthHit == null ||
      effectivePriority == null ||
      effectiveOneShot == null
    ) {
      return;
    }
    return {
      template_id: selected.template_id,
      existing_rule_id: null,
      expected_revision: null,
      channel: effectiveChannel,
      terminal: terminal || null,
      target: target || null,
      nth_hit: effectiveNthHit,
      one_shot: effectiveOneShot,
      priority: effectivePriority,
      parameters,
    };
  }, [
    effectiveNthHit,
    effectiveOneShot,
    effectivePriority,
    effectiveChannel,
    parameters,
    selected,
    target,
    terminal,
  ]);

  function setParameter(key: string, value: FaultParameterValue) {
    if (!selected) return;
    setFieldErrors((current) => {
      const next = { ...current };
      delete next[`parameters.${key}`];
      return next;
    });
    setParameterOverrides((current) => ({
      ...current,
      [selected.template_id]: {
        ...(current[selected.template_id] ?? selected.default_parameters),
        [key]: value,
      },
    }));
  }

  function clearFieldError(field: string) {
    setFieldErrors((current) => {
      const next = { ...current };
      delete next[field];
      return next;
    });
  }

  function selectTemplate(templateId: string) {
    // 切换模板时恢复该模板默认值；每个模板的临时参数覆盖相互隔离。
    setSelectedId(templateId);
    const template = templates.data?.find(
      (item) => item.template_id === templateId,
    );
    setNthHit(template?.default_nth_hit);
    setPriority(template?.default_priority);
    setOneShot(template?.default_one_shot);
    // Rust 已把模板默认值规范化为当前 Workspace 的真实 Listener UUID。
    setChannel(template?.default_channel);
    if (window.matchMedia("(max-width: 1280px)").matches) {
      requestAnimationFrame(() => {
        configurationPanelRef.current?.scrollIntoView({ block: "start" });
      });
    }
  }

  async function configure(openRules = false) {
    // 两个入口共用同一 Rust Command；openRules 只决定成功后是否跳到规则页。
    if (!draft || writePending) return;
    setConfigurePending(openRules ? "save" : "enable");
    setFieldErrors({});
    try {
      const result = await callCommand(commands.faultConfigure(draft));
      toast(result.status_text, { variant: toneColor(result.ui_tone) });
      await active.refresh();
      if (openRules) navigate("/rules");
    } catch (reason) {
      setFieldErrors(appErrorViewModel(reason)?.field_errors ?? {});
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setConfigurePending(undefined);
    }
  }

  async function stopFault(item: ActiveFaultViewModel) {
    if (writePending) return;
    setStoppingRuleId(item.rule_id);
    try {
      const result = await callCommand(
        commands.faultStop(item.rule_id, item.revision, true),
      );
      toast(result.status_text, { variant: toneColor(result.ui_tone) });
      await active.refresh();
      setStopDialogRuleId(undefined);
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setStoppingRuleId(undefined);
    }
  }

  return (
    <section className="grid h-full grid-cols-[minmax(0,1fr)_430px] max-[1280px]:grid-cols-1">
      <div className="min-w-0 space-y-4 overflow-auto p-5">
        <h1 className="text-2xl font-semibold">故障模拟</h1>
        <Alert status="accent">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>故障模板最终创建普通拦截规则</Alert.Title>
            <Alert.Description>
              复杂条件可在规则管理继续编辑，不建立第二套执行引擎。
            </Alert.Description>
          </Alert.Content>
        </Alert>
        {channelCatalog.length === 0 && (
          <Alert status="warning">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>当前 Workspace 没有代理入口</Alert.Title>
              <Alert.Description>
                请先在“代理入口配置”中新增入口，故障模拟才能绑定到实际流量通道。
              </Alert.Description>
            </Alert.Content>
          </Alert>
        )}

        <div>
          <h2 className="mb-3 text-lg font-semibold">
            故障模板（快速启用安全的故障场景）
          </h2>
          {templates.error && (
            <Alert status="danger" className="mb-3">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>故障模板读取失败</Alert.Title>
                <Alert.Description>{templates.error}</Alert.Description>
              </Alert.Content>
              <Button
                size="sm"
                variant="outline"
                onPress={() => void templates.refresh()}
              >
                重试
              </Button>
            </Alert>
          )}
          <Table>
            <Table.ScrollContainer>
              <Table.Content
                aria-label="故障模板"
                className="min-w-[820px]"
                selectionMode="single"
                selectedKeys={effectiveSelectedId ? [effectiveSelectedId] : []}
                onSelectionChange={(keys) => {
                  if (keys === "all") return;
                  const first = Array.from(keys)[0];
                  if (first != null) selectTemplate(String(first));
                }}
              >
                <Table.Header>
                  <Table.Column>阶段</Table.Column>
                  <Table.Column isRowHeader>行为（精确语义）</Table.Column>
                  <Table.Column>影响端</Table.Column>
                  <Table.Column>默认参数</Table.Column>
                  <Table.Column>风险</Table.Column>
                </Table.Header>
                <Table.Body
                  renderEmptyState={() => (
                    <div className="p-8 text-center">
                      {templates.isLoading
                        ? "正在读取故障模板…"
                        : templates.error
                          ? "故障模板暂不可用"
                          : "暂无故障模板"}
                    </div>
                  )}
                >
                  {(templates.data ?? []).map((template) => (
                    <Table.Row
                      key={template.template_id}
                      id={template.template_id}
                    >
                      <Table.Cell>{template.stage_text}</Table.Cell>
                      <Table.Cell>
                        <div className="font-medium">{template.name}</div>
                        <div className="text-xs text-[var(--telemetry-muted)]">
                          {template.behavior_text}
                        </div>
                      </Table.Cell>
                      <Table.Cell>{template.affected_party_text}</Table.Cell>
                      <Table.Cell className="max-w-56 text-xs">
                        {Object.entries(template.default_parameters)
                          .map(([key, value]) => `${key}: ${value.value}`)
                          .join("；") || "—"}
                      </Table.Cell>
                      <Table.Cell>
                        <Chip
                          size="sm"
                          color={toneColor(template.ui_tone)}
                          variant="soft"
                        >
                          {template.risk_text}
                        </Chip>
                      </Table.Cell>
                    </Table.Row>
                  ))}
                </Table.Body>
              </Table.Content>
            </Table.ScrollContainer>
          </Table>
        </div>

        <div>
          <h2 className="mb-3 text-lg font-semibold">当前生效的模拟</h2>
          {active.error && (
            <Alert status="danger" className="mb-3">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>活动模拟读取失败</Alert.Title>
                <Alert.Description>{active.error}</Alert.Description>
              </Alert.Content>
              <Button
                size="sm"
                variant="outline"
                onPress={() => void active.refresh()}
              >
                重试
              </Button>
            </Alert>
          )}
          <Table>
            <Table.ScrollContainer>
              <Table.Content
                aria-label="当前活动故障模拟"
                className="min-w-[760px]"
              >
                <Table.Header>
                  <Table.Column isRowHeader>模板</Table.Column>
                  <Table.Column>目标</Table.Column>
                  <Table.Column>规则优先级</Table.Column>
                  <Table.Column>命中次数</Table.Column>
                  <Table.Column>状态</Table.Column>
                  <Table.Column>停用</Table.Column>
                </Table.Header>
                <Table.Body
                  renderEmptyState={() => (
                    <div className="p-8 text-center">
                      {active.isLoading
                        ? "正在读取活动模拟…"
                        : active.error
                          ? "活动模拟暂不可用"
                          : "当前没有活动模拟"}
                    </div>
                  )}
                >
                  {(active.data ?? []).map((item) => (
                    <Table.Row key={item.rule_id} id={item.rule_id}>
                      <Table.Cell>{item.template_name}</Table.Cell>
                      <Table.Cell>{item.target_summary}</Table.Cell>
                      <Table.Cell>{item.priority}</Table.Cell>
                      <Table.Cell>{item.hit_count}</Table.Cell>
                      <Table.Cell>
                        <Chip
                          size="sm"
                          color={toneColor(item.ui_tone)}
                          variant="soft"
                        >
                          {item.status_text}
                        </Chip>
                      </Table.Cell>
                      <Table.Cell>
                        <AlertDialog
                          isOpen={stopDialogRuleId === item.rule_id}
                          onOpenChange={(open) => {
                            if (!open && stoppingRuleId === item.rule_id) return;
                            setStopDialogRuleId(
                              open ? item.rule_id : undefined,
                            );
                          }}
                        >
                          <Button
                            size="sm"
                            variant="danger-soft"
                            isDisabled={writePending}
                          >
                            停用
                          </Button>
                          <AlertDialog.Backdrop>
                            <AlertDialog.Container>
                              <AlertDialog.Dialog>
                                <AlertDialog.Header>
                                  <AlertDialog.Heading>
                                    停止此故障模拟？
                                  </AlertDialog.Heading>
                                </AlertDialog.Header>
                                <AlertDialog.Body>
                                  Rust 将停用对应的普通拦截规则。
                                </AlertDialog.Body>
                                <AlertDialog.Footer>
                                  <Button
                                    slot="close"
                                    variant="outline"
                                    isDisabled={
                                      stoppingRuleId === item.rule_id
                                    }
                                  >
                                    取消
                                  </Button>
                                  <Button
                                    variant="danger"
                                    isDisabled={stoppingRuleId === item.rule_id}
                                    onPress={() => void stopFault(item)}
                                  >
                                    {stoppingRuleId === item.rule_id
                                      ? "正在停用…"
                                      : "确认停用"}
                                  </Button>
                                </AlertDialog.Footer>
                              </AlertDialog.Dialog>
                            </AlertDialog.Container>
                          </AlertDialog.Backdrop>
                        </AlertDialog>
                      </Table.Cell>
                    </Table.Row>
                  ))}
                </Table.Body>
              </Table.Content>
            </Table.ScrollContainer>
          </Table>
        </div>
      </div>

      <aside
        ref={configurationPanelRef}
        className="scroll-mt-4 overflow-auto border-l border-[var(--telemetry-line)] p-5 max-[1280px]:border-l-0 max-[1280px]:border-t"
      >
        <h2 className="text-lg font-semibold">
          配置模板：{selected?.name ?? "未选择"}
        </h2>
        {selected && (
          <div className="mt-4 space-y-5">
            <Card>
              <Card.Header>
                <Card.Title>精确行为序列（网络语义）</Card.Title>
              </Card.Header>
              <Card.Content>
                <p className="text-sm">{selected.behavior_text}</p>
              </Card.Content>
            </Card>
            {selected.parameter_schema.map((field) => {
              const value = parameters[field.key];
              if (field.kind === "boolean") {
                const error = fieldError(`parameters.${field.key}`);
                return (
                  <div key={field.key}>
                    <Switch
                      aria-label={field.label}
                      isSelected={
                        value?.kind === "boolean" ? value.value : false
                      }
                      onChange={(next) =>
                        setParameter(field.key, {
                          kind: "boolean",
                          value: next,
                        })
                      }
                    >
                      <Switch.Control>
                        <Switch.Thumb />
                      </Switch.Control>
                      <Switch.Content>
                        <span>{field.label}</span>
                        <span className="block text-xs text-[var(--telemetry-muted)]">
                          {field.description}
                        </span>
                      </Switch.Content>
                    </Switch>
                    {error && <FieldError>{error}</FieldError>}
                  </div>
                );
              }
              if (field.kind === "integer") {
                const error = fieldError(`parameters.${field.key}`);
                return (
                  <div key={field.key}>
                    <NumberField
                      isInvalid={Boolean(error)}
                      value={value?.kind === "integer" ? value.value : 0}
                      minValue={field.minimum ?? undefined}
                      maxValue={field.maximum ?? undefined}
                      onChange={(next) =>
                        setParameter(field.key, {
                          kind: "integer",
                          value: next,
                        })
                      }
                    >
                      <Label>{field.label}</Label>
                      <NumberField.Group className="w-full">
                        <NumberField.DecrementButton />
                        <NumberField.Input />
                        <NumberField.IncrementButton />
                      </NumberField.Group>
                      {error && <FieldError>{error}</FieldError>}
                    </NumberField>
                    <p className="mt-1 text-xs text-[var(--telemetry-muted)]">
                      {field.description}
                    </p>
                  </div>
                );
              }
              const text =
                value?.kind === "text" || value?.kind === "json"
                  ? value.value
                  : "";
              const nextKind = field.kind === "json" ? "json" : "text";
              if (field.multiline) {
                const error = fieldError(`parameters.${field.key}`);
                return (
                  <TextField key={field.key} isInvalid={Boolean(error)}>
                    <Label>{field.label}</Label>
                    <TextArea
                      aria-label={field.label}
                      className={
                        field.kind === "json"
                          ? "mt-1 min-h-32 font-mono text-xs"
                          : "mt-1 min-h-32"
                      }
                      value={text}
                      onChange={(event) =>
                        setParameter(field.key, {
                          kind: nextKind,
                          value: event.target.value,
                        })
                      }
                    />
                    <p className="mt-1 text-xs text-[var(--telemetry-muted)]">
                      {field.description}
                    </p>
                    {error && <FieldError>{error}</FieldError>}
                  </TextField>
                );
              }
              const error = fieldError(`parameters.${field.key}`);
              return (
                <TextField key={field.key} isInvalid={Boolean(error)}>
                  <Label>{field.label}</Label>
                  <Input
                    value={text}
                    onChange={(event) =>
                      setParameter(field.key, {
                        kind: nextKind,
                        value: event.target.value,
                      })
                    }
                  />
                  <p className="text-xs text-[var(--telemetry-muted)]">
                    {field.description}
                  </p>
                  {error && <FieldError>{error}</FieldError>}
                </TextField>
              );
            })}
            <Alert
              status={selected.ui_tone === "danger" ? "danger" : "warning"}
            >
              {selected.risk_text}
            </Alert>
            <div className="grid gap-1">
              <Label>代理通道</Label>
              <Select
                aria-label="代理通道"
                selectedKey={effectiveChannel}
                onSelectionChange={(value) => {
                  if (value != null) {
                    setChannel(value as ChannelId);
                  }
                }}
              >
                <Select.Trigger>
                  <Select.Value />
                  <Select.Indicator />
                </Select.Trigger>
                <Select.Popover>
                  <ListBox>
                    {channelCatalog.map((catalogItem) => (
                      <ListBox.Item
                        key={catalogItem.id}
                        id={catalogItem.id}
                        textValue={catalogItem.display_name}
                      >
                        {catalogItem.display_name}
                      </ListBox.Item>
                    ))}
                  </ListBox>
                </Select.Popover>
              </Select>
            </div>
            <TextField isInvalid={Boolean(fieldError("terminal"))}>
              <Label>终端过滤（可选）</Label>
              <Input
                aria-label="终端过滤（可选）"
                value={terminal}
                placeholder="按终端 ID 或 IP"
                onChange={(event) => {
                  clearFieldError("terminal");
                  setTerminal(event.target.value);
                }}
              />
              {fieldError("terminal") && (
                <FieldError>{fieldError("terminal")}</FieldError>
              )}
            </TextField>
            <TextField isInvalid={Boolean(fieldError("target"))}>
              <Label>路径与请求类型</Label>
              <Input
                aria-label="路径与请求类型"
                value={target}
                placeholder="/v1/resources/example"
                onChange={(event) => {
                  clearFieldError("target");
                  setTarget(event.target.value);
                }}
              />
              {fieldError("target") && (
                <FieldError>{fieldError("target")}</FieldError>
              )}
            </TextField>
            <NumberField
              isInvalid={Boolean(fieldError("nth_hit"))}
              value={effectiveNthHit}
              minValue={1}
              onChange={(value) => {
                clearFieldError("nth_hit");
                setNthHit(value);
              }}
            >
              <Label>第 N 次命中</Label>
              <NumberField.Group className="w-full">
                <NumberField.DecrementButton />
                <NumberField.Input />
                <NumberField.IncrementButton />
              </NumberField.Group>
              {fieldError("nth_hit") && (
                <FieldError>{fieldError("nth_hit")}</FieldError>
              )}
            </NumberField>
            <Switch
              aria-label="一次性生效"
              isSelected={effectiveOneShot}
              onChange={setOneShot}
            >
              <Switch.Control>
                <Switch.Thumb />
              </Switch.Control>
              <Switch.Content>一次性生效（命中后自动停用）</Switch.Content>
            </Switch>
            <NumberField
              isInvalid={Boolean(fieldError("priority"))}
              value={effectivePriority}
              onChange={(value) => {
                clearFieldError("priority");
                setPriority(value);
              }}
            >
              <Label>规则优先级</Label>
              <NumberField.Group className="w-full">
                <NumberField.DecrementButton />
                <NumberField.Input />
                <NumberField.IncrementButton />
              </NumberField.Group>
              {fieldError("priority") && (
                <FieldError>{fieldError("priority")}</FieldError>
              )}
            </NumberField>
            <div className="flex gap-3">
              <Button
                variant="primary"
                isDisabled={writePending || !draft}
                onPress={() => void configure(false)}
              >
                {configurePending === "enable"
                  ? "正在启用…"
                  : "启用模拟"}
              </Button>
              <Button
                variant="outline"
                isDisabled={writePending || !draft}
                onPress={() => void configure(true)}
              >
                {configurePending === "save"
                  ? "正在保存…"
                  : "保存为规则"}
              </Button>
            </div>
          </div>
        )}
      </aside>
    </section>
  );
}
