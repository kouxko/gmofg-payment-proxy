import { useState } from "react";
import { Button, Input, Label, ListBox, Select, TextField } from "@heroui/react";
import type { ProxyListener, RuleDefinitionSaveInput, RuleEditorContext, RuleNewDefinitionDraft } from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { ruleStageLabel } from "./rule-definition-model";

export function RuleCreationDialog(props: {
  open: boolean;
  listeners: ProxyListener[];
  onClose: () => void;
  onCreate: (draft: RuleDefinitionSaveInput, context: RuleEditorContext) => void;
}) {
  const [context, setContext] = useState<RuleEditorContext>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>();
  const [structure, setStructure] = useState<RuleNewDefinitionDraft>();
  const [name, setName] = useState("");
  const [enabled, setEnabled] = useState<"true" | "false" | "">("");
  const [priority, setPriority] = useState("");
  const [oneShot, setOneShot] = useState<"true" | "false" | "">("");
  if (!props.open) return null;
  const stages = context?.content.value.stages ?? [];

  async function selectListener(listenerId: string) {
    setContext(undefined);
    setStructure(undefined);
    setError(undefined);
    if (!listenerId) return;
    setLoading(true);
    try {
      setContext(await callCommand(commands.ruleEditorContext(listenerId)));
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setLoading(false);
    }
  }

  function create() {
    if (!context || !structure || name.trim() === "" || enabled === "" || oneShot === "") return;
    const parsedPriority = Number(priority);
    if (!Number.isSafeInteger(parsedPriority) || parsedPriority < 0) return;
    props.onCreate({
      rule_id: null,
      expected_revision: null,
      draft: {
        name,
        enabled: enabled === "true",
        priority: parsedPriority,
        listener_id: structure.listener_id,
        stage: structure.stage,
        one_shot: oneShot === "true",
        content: structure.content,
      },
    }, context);
  }

  return (
    <div aria-label="创建统一规则" className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4" role="dialog">
      <div className="w-full max-w-xl space-y-4 rounded-xl bg-[var(--telemetry-surface)] p-5 shadow-xl">
        <header className="flex items-center"><h2 className="text-lg font-semibold">新建规则</h2><Button className="ml-auto" variant="ghost" onPress={props.onClose}>关闭</Button></header>
        <Select aria-label="创建规则的 Listener" isDisabled={loading || props.listeners.length === 0} selectedKey={context?.listener_id ?? null} onSelectionChange={(key) => void selectListener(String(key))}>
          <Select.Trigger><Select.Value>{({ selectedText }) => selectedText || "选择 Listener"}</Select.Value><Select.Indicator /></Select.Trigger>
          <Select.Popover><ListBox>{props.listeners.map((listener) => <ListBox.Item id={listener.id} key={listener.id} textValue={`${listener.name} · ${listener.data_plane.kind === "http" ? "HTTP" : "Socket"}`}>{listener.name} · {listener.data_plane.kind === "http" ? "HTTP" : "Socket"}</ListBox.Item>)}</ListBox></Select.Popover>
        </Select>
        {props.listeners.length === 0 && <p className="text-sm">当前 Workspace 没有可用于创建规则的 Listener。</p>}
        {loading && <p>正在读取 Rust 规则能力…</p>}
        {error && <p role="alert" className="text-sm text-red-600">{error}</p>}
        {context && <section className="space-y-2"><h3 className="font-medium">选择处理阶段</h3><div className="flex flex-wrap gap-2">
          {stages.map((stage) => <Button key={stage.stage} variant="outline" onPress={() => setStructure(stage.new_rule_draft)}>{ruleStageLabel(stage.stage)}</Button>)}
        </div></section>}
        {structure && <section className="grid gap-2 sm:grid-cols-2">
          <TextField><Label>规则名称</Label><Input aria-label="新规则名称" value={name} onChange={(event) => setName(event.target.value)} /></TextField>
          <TextField><Label>阶段内优先级</Label><Input aria-label="新规则优先级" inputMode="numeric" value={priority} onChange={(event) => setPriority(event.target.value)} /></TextField>
          <Select aria-label="新规则是否启用" selectedKey={enabled || null} onSelectionChange={(key) => setEnabled(String(key) as typeof enabled)}><Label>是否启用</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox><ListBox.Item id="true" textValue="启用">启用</ListBox.Item><ListBox.Item id="false" textValue="不启用">不启用</ListBox.Item></ListBox></Select.Popover></Select>
          <Select aria-label="新规则是否单次" selectedKey={oneShot || null} onSelectionChange={(key) => setOneShot(String(key) as typeof oneShot)}><Label>是否单次</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox><ListBox.Item id="true" textValue="单次">单次</ListBox.Item><ListBox.Item id="false" textValue="持续">持续</ListBox.Item></ListBox></Select.Popover></Select>
          <Button isDisabled={name.trim() === "" || enabled === "" || oneShot === "" || !Number.isSafeInteger(Number(priority)) || Number(priority) < 0} variant="primary" onPress={create}>进入规则编辑器</Button>
        </section>}
      </div>
    </div>
  );
}
