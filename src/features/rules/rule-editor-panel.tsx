import type { RefObject } from "react";
import {
  Alert,
  AlertDialog,
  Button,
  FieldError,
  Form,
  Input,
  Label,
  ListBox,
  NumberField,
  Select,
  Spinner,
  Switch,
  Tabs,
  TextArea,
  TextField,
} from "@heroui/react";
import { Copy, TrashBin } from "@gravity-ui/icons";
import type {
  ChannelPresentationViewModel,
  RuleDraft,
  RuleStageCapabilityViewModel,
} from "@/generated/rust-types";
import {
  ActionsEditor,
  ConditionsEditor,
  type RuleDraftChange,
} from "./rule-editor";

type AsyncState = { pending: boolean; invalid: boolean };
type RuleEditorPanelProps = {
  panelRef: RefObject<HTMLElement | null>;
  draft?: RuleDraft;
  isLoading: boolean;
  loadError?: string;
  fieldErrors: Record<string, string[]>;
  channelCatalog: ChannelPresentationViewModel[];
  capabilities?: RuleStageCapabilityViewModel[];
  capabilityError?: string;
  writePending: boolean;
  editorBlocked: boolean;
  pendingAction?: string;
  selectedId?: string;
  deleteDialogOpen: boolean;
  deletePending: boolean;
  onDraftChange: (change: RuleDraftChange) => void;
  onAsyncStateChange: (key: string, state?: AsyncState) => void;
  onRetry: () => void;
  onSave: () => void;
  onCopy: () => void;
  onDelete: () => void;
  onDeleteDialogChange: (open: boolean) => void;
};

export function RuleEditorPanel({
  panelRef,
  draft,
  isLoading,
  loadError,
  fieldErrors,
  channelCatalog,
  capabilities,
  capabilityError,
  writePending,
  editorBlocked,
  pendingAction,
  selectedId,
  deleteDialogOpen,
  deletePending,
  onDraftChange,
  onAsyncStateChange,
  onRetry,
  onSave,
  onCopy,
  onDelete,
  onDeleteDialogChange,
}: RuleEditorPanelProps) {
  const fieldError = (field: string) => fieldErrors[field]?.join("；");
  const stageCapability = capabilities?.find(
    (capability) => capability.stage === draft?.stage,
  );
  return (
    <aside
      ref={panelRef}
      className="scroll-mt-4 overflow-auto border-l border-[var(--telemetry-line)] p-5 [scrollbar-gutter:stable] max-[1280px]:border-l-0 max-[1280px]:border-t"
    >
      <h2 className="mb-4 text-lg font-semibold">
        {draft
          ? `编辑规则：${draft.name}`
          : isLoading
            ? "正在读取规则…"
            : "选择规则进行编辑"}
      </h2>
      {isLoading && !draft && (
        <div className="grid min-h-40 place-items-center">
          <Spinner aria-label="正在读取规则详情" />
        </div>
      )}
      {loadError && !draft && (
        <Alert status="danger">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>规则详情读取失败</Alert.Title>
            <Alert.Description>{loadError}</Alert.Description>
          </Alert.Content>
          <Button size="sm" variant="outline" onPress={onRetry}>
            重试
          </Button>
        </Alert>
      )}
      {draft && (
        <Form className="space-y-4">
          <Tabs defaultSelectedKey="basic">
            <Tabs.ListContainer>
              <Tabs.List aria-label="规则编辑">
                <Tabs.Tab id="basic">
                  基本信息
                  <Tabs.Indicator />
                </Tabs.Tab>
                <Tabs.Tab id="conditions">
                  匹配条件
                  <Tabs.Indicator />
                </Tabs.Tab>
                <Tabs.Tab id="actions">
                  执行动作
                  <Tabs.Indicator />
                </Tabs.Tab>
              </Tabs.List>
            </Tabs.ListContainer>
            <Tabs.Panel id="basic" className="space-y-4 pt-4">
              <TextField isInvalid={Boolean(fieldError("name"))}>
                <Label>规则名称</Label>
                <Input
                  value={draft.name}
                  onChange={(event) =>
                    onDraftChange({ ...draft, name: event.target.value })
                  }
                />
                {fieldError("name") && (
                  <FieldError>{fieldError("name")}</FieldError>
                )}
              </TextField>
              <TextField>
                <Label>规则说明</Label>
                <TextArea
                  value={draft.description}
                  onChange={(event) =>
                    onDraftChange({ ...draft, description: event.target.value })
                  }
                />
              </TextField>
              <NumberField
                value={draft.priority}
                onChange={(priority) => onDraftChange({ ...draft, priority })}
              >
                <Label>规则优先级</Label>
                <NumberField.Group className="w-full">
                  <NumberField.DecrementButton />
                  <NumberField.Input />
                  <NumberField.IncrementButton />
                </NumberField.Group>
              </NumberField>
              <RuleSelects
                draft={draft}
                channels={channelCatalog}
                onChange={onDraftChange}
              />
              <div className="flex flex-wrap gap-5">
                <Switch
                  aria-label="启用规则"
                  isSelected={draft.enabled}
                  onChange={(enabled) => onDraftChange({ ...draft, enabled })}
                >
                  <Switch.Content>
                    <Switch.Control>
                      <Switch.Thumb />
                    </Switch.Control>
                    <span>启用规则</span>
                  </Switch.Content>
                </Switch>
                <Switch
                  aria-label="仅命中一次"
                  isSelected={draft.one_shot}
                  onChange={(one_shot) => onDraftChange({ ...draft, one_shot })}
                >
                  <Switch.Content>
                    <Switch.Control>
                      <Switch.Thumb />
                    </Switch.Control>
                    <span>仅命中一次</span>
                  </Switch.Content>
                </Switch>
              </div>
            </Tabs.Panel>
            <Tabs.Panel id="conditions" className="pt-4">
              {stageCapability ? (
                <ConditionsEditor
                  draft={draft}
                  fieldErrors={fieldErrors}
                  onChange={onDraftChange}
                  onAsyncStateChange={onAsyncStateChange}
                  capability={stageCapability}
                />
              ) : (
                <CapabilityUnavailable error={capabilityError} />
              )}
              <p className="mt-2 text-xs text-[var(--telemetry-muted)]">
                空条件表示匹配该通道和阶段的全部消息；保存时统一校验。
              </p>
            </Tabs.Panel>
            <Tabs.Panel id="actions" className="pt-4">
              {stageCapability ? (
                <ActionsEditor
                  draft={draft}
                  fieldErrors={fieldErrors}
                  onChange={onDraftChange}
                  onAsyncStateChange={onAsyncStateChange}
                  capability={stageCapability}
                />
              ) : (
                <CapabilityUnavailable error={capabilityError} />
              )}
              <p className="mt-2 text-xs text-[var(--telemetry-muted)]">
                动作顺序即执行顺序，终止动作会中断后续评估。
              </p>
            </Tabs.Panel>
          </Tabs>
          <Alert status="success">
            保存时会校验字段、正则、JSON 路径和动作兼容性。
          </Alert>
          {Object.keys(fieldErrors).length > 0 && (
            <Alert status="danger">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>规则配置校验失败</Alert.Title>
                <Alert.Description>
                  {Object.values(fieldErrors).flat().join("；")}
                </Alert.Description>
              </Alert.Content>
            </Alert>
          )}
          <EditorActions
            writePending={writePending}
            editorBlocked={editorBlocked}
            pendingAction={pendingAction}
            selectedId={selectedId}
            deleteDialogOpen={deleteDialogOpen}
            deletePending={deletePending}
            onSave={onSave}
            onCopy={onCopy}
            onDelete={onDelete}
            onDeleteDialogChange={onDeleteDialogChange}
          />
        </Form>
      )}
    </aside>
  );
}

function CapabilityUnavailable({ error }: { error?: string }) {
  return (
    <Alert status="danger">
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>规则能力读取失败</Alert.Title>
        <Alert.Description>
          {error ?? "当前阶段没有可用的规则能力，请重新选择阶段。"}
        </Alert.Description>
      </Alert.Content>
    </Alert>
  );
}

function RuleSelects({
  draft,
  channels,
  onChange,
}: {
  draft: RuleDraft;
  channels: ChannelPresentationViewModel[];
  onChange: (change: RuleDraftChange) => void;
}) {
  return (
    <>
      <div className="grid gap-1">
        <Label>通道</Label>
        <Select
          aria-label="规则通道"
          selectedKey={draft.channel ?? undefined}
          onSelectionChange={(key) =>
            onChange({
              ...draft,
              channel: key as RuleDraft["channel"],
            })
          }
        >
          <Select.Trigger>
            <Select.Value />
            <Select.Indicator />
          </Select.Trigger>
          <Select.Popover>
            <ListBox>
              {channels.map((channel) => (
                <ListBox.Item
                  key={channel.id}
                  id={channel.id}
                  textValue={channel.display_name}
                >
                  {channel.display_name}
                </ListBox.Item>
              ))}
            </ListBox>
          </Select.Popover>
        </Select>
      </div>
      <div className="grid gap-1">
        <Label>阶段</Label>
        <Select
          aria-label="规则阶段"
          selectedKey={draft.stage ?? "none"}
          onSelectionChange={(key) =>
            onChange({
              ...draft,
              stage: key === "none" ? null : (key as RuleDraft["stage"]),
            })
          }
        >
          <Select.Trigger>
            <Select.Value />
            <Select.Indicator />
          </Select.Trigger>
          <Select.Popover>
            <ListBox>
              <ListBox.Item id="none" textValue="请选择">请选择</ListBox.Item>
              <ListBox.Item id="tls_handshake" textValue="TLS 握手">
                TLS 握手
              </ListBox.Item>
              <ListBox.Item id="request" textValue="请求">请求</ListBox.Item>
              <ListBox.Item id="response" textValue="响应">响应</ListBox.Item>
            </ListBox>
          </Select.Popover>
        </Select>
      </div>
    </>
  );
}

type EditorActionsProps = Pick<
  RuleEditorPanelProps,
  | "writePending"
  | "editorBlocked"
  | "pendingAction"
  | "selectedId"
  | "deleteDialogOpen"
  | "deletePending"
  | "onSave"
  | "onCopy"
  | "onDelete"
  | "onDeleteDialogChange"
>;

function EditorActions(props: EditorActionsProps) {
  return (
    <div className="flex gap-3">
      <Button
        variant="primary"
        isDisabled={props.writePending || props.editorBlocked}
        onPress={props.onSave}
      >
        {props.pendingAction === "save"
          ? "正在保存…"
          : props.editorBlocked
            ? "正在解析输入"
            : "保存规则"}
      </Button>
      <Button
        variant="outline"
        isDisabled={
          !props.selectedId || props.writePending || props.editorBlocked
        }
        onPress={props.onCopy}
      >
        <Copy className="size-4" />
        {props.pendingAction === "copy" ? "正在复制…" : "复制规则"}
      </Button>
      <AlertDialog
        isOpen={props.deleteDialogOpen}
        onOpenChange={props.onDeleteDialogChange}
      >
        <Button
          variant="danger-soft"
          isDisabled={
            !props.selectedId || props.writePending || props.editorBlocked
          }
        >
          <TrashBin className="size-4" />
          删除规则
        </Button>
        <AlertDialog.Backdrop>
          <AlertDialog.Container>
            <AlertDialog.Dialog>
              <AlertDialog.Header>
                <AlertDialog.Heading>删除此规则？</AlertDialog.Heading>
              </AlertDialog.Header>
              <AlertDialog.Body>删除后无法恢复。</AlertDialog.Body>
              <AlertDialog.Footer>
                <Button
                  slot="close"
                  variant="outline"
                  isDisabled={props.deletePending}
                >
                  取消
                </Button>
                <Button
                  variant="danger"
                  isDisabled={props.deletePending}
                  onPress={props.onDelete}
                >
                  {props.deletePending ? "正在删除…" : "确认删除"}
                </Button>
              </AlertDialog.Footer>
            </AlertDialog.Dialog>
          </AlertDialog.Container>
        </AlertDialog.Backdrop>
      </AlertDialog>
    </div>
  );
}
