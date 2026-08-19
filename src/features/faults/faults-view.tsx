"use client";

/**
 * HTTP 规则页的故障预设容器。
 *
 * 本文件只负责 Rust IPC、表单草稿和异步状态；模板列表与配置表单分别由
 * 独立展示组件负责，避免把查询、业务动作和大量 JSX 混在一个文件中。
 */

import { useMemo, useRef, useState } from "react";
import { toast } from "@heroui/react";
import type {
  ChannelId,
  FaultConfigurationDraft,
  FaultParameterValue,
  FaultTemplateViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { appErrorViewModel, callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { toneColor } from "@/lib/format";
import { useBootstrap } from "@/features/shell/bootstrap-context";
import { FaultConfigurationPanel } from "./fault-configuration-panel";
import { FaultsListPanel } from "./faults-list-panel";

export function FaultPresetsView({
  onRuleCreated,
}: {
  onRuleCreated?: (ruleId: string) => void;
}) {
  const { bootstrap } = useBootstrap();
  const channels = bootstrap?.channel_catalog ?? [];
  const templates = useIpcQuery<FaultTemplateViewModel[]>(
    "fault-template-list",
    () => callCommand(commands.faultTemplateList()),
  );
  const [selectedId, setSelectedId] = useState<string>();
  const [configurePending, setConfigurePending] = useState(false);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string[]>>({});
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

  const effectiveSelectedId = selectedId ?? templates.data?.[0]?.template_id;
  const selected = templates.data?.find(
    (item) => item.template_id === effectiveSelectedId,
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
  const writePending = configurePending;
  const fieldError = (field: string) => fieldErrors[field]?.join("；");

  const draft = useMemo<FaultConfigurationDraft | undefined>(() => {
    if (
      !selected ||
      effectiveChannel == null ||
      effectiveNthHit == null ||
      effectivePriority == null ||
      effectiveOneShot == null
    )
      return;
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
    effectiveChannel,
    effectiveNthHit,
    effectiveOneShot,
    effectivePriority,
    parameters,
    selected,
    target,
    terminal,
  ]);

  function clearFieldError(field: string) {
    setFieldErrors((current) => {
      const next = { ...current };
      delete next[field];
      return next;
    });
  }

  function setParameter(key: string, value: FaultParameterValue) {
    if (!selected) return;
    clearFieldError(`parameters.${key}`);
    setParameterOverrides((current) => ({
      ...current,
      [selected.template_id]: {
        ...(current[selected.template_id] ?? selected.default_parameters),
        [key]: value,
      },
    }));
  }

  function selectTemplate(templateId: string) {
    setSelectedId(templateId);
    const template = templates.data?.find(
      (item) => item.template_id === templateId,
    );
    setNthHit(template?.default_nth_hit);
    setPriority(template?.default_priority);
    setOneShot(template?.default_one_shot);
    setChannel(template?.default_channel);
    if (window.matchMedia("(max-width: 1280px)").matches) {
      requestAnimationFrame(() =>
        configurationPanelRef.current?.scrollIntoView({ block: "start" }),
      );
    }
  }

  async function configure() {
    if (!draft || writePending) return;
    setConfigurePending(true);
    setFieldErrors({});
    try {
      const result = await callCommand(commands.faultConfigure(draft));
      toast(result.status_text, { variant: toneColor(result.ui_tone) });
      onRuleCreated?.(result.rule_id);
    } catch (reason) {
      setFieldErrors(appErrorViewModel(reason)?.field_errors ?? {});
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setConfigurePending(false);
    }
  }

  return (
    <section className="grid h-full grid-cols-[minmax(0,1fr)_430px] max-[1280px]:grid-cols-1">
      <FaultsListPanel
        templates={templates}
        effectiveSelectedId={effectiveSelectedId}
        hasChannels={channels.length > 0}
        onSelectTemplate={selectTemplate}
      />
      <FaultConfigurationPanel
        panelRef={configurationPanelRef}
        selected={selected}
        parameters={parameters}
        channels={channels}
        channel={effectiveChannel}
        terminal={terminal}
        target={target}
        nthHit={effectiveNthHit}
        priority={effectivePriority}
        oneShot={effectiveOneShot}
        draft={draft}
        configurePending={configurePending}
        writePending={writePending}
        fieldError={fieldError}
        onSetParameter={setParameter}
        onChannelChange={setChannel}
        onTerminalChange={(value) => {
          clearFieldError("terminal");
          setTerminal(value);
        }}
        onTargetChange={(value) => {
          clearFieldError("target");
          setTarget(value);
        }}
        onNthHitChange={(value) => {
          clearFieldError("nth_hit");
          setNthHit(value);
        }}
        onPriorityChange={(value) => {
          clearFieldError("priority");
          setPriority(value);
        }}
        onOneShotChange={setOneShot}
        onConfigure={() => void configure()}
      />
    </section>
  );
}
