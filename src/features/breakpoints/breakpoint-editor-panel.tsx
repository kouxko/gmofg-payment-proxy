import {
  Alert,
  Button,
  Drawer,
  Spinner,
  Tabs,
  TextArea,
} from "@heroui/react";
import { Copy } from "@gravity-ui/icons";
import type {
  BreakpointDecision,
  BreakpointDetailViewModel,
  FieldValidationViewModel,
} from "@/generated/rust-types";
import { HttpBodyViewer, HttpRequestTargetView } from "@/features/shared/http-inspection";
import { messageContentLabel } from "@/lib/message-content";
import type { BreakpointActionPanelProps } from "./breakpoint-action-panel";
import { BreakpointActionPanel } from "./breakpoint-action-panel";

interface DetailQuery {
  data?: BreakpointDetailViewModel;
  error?: string;
  isLoading: boolean;
  refresh: () => Promise<void>;
}
interface BreakpointEditorPanelProps {
  hasSelection: boolean;
  detail: DetailQuery;
  bodyText: string;
  editorPending?: "format" | "restore" | "validate";
  resolvePending: boolean;
  validation?: FieldValidationViewModel;
  validationError: (field: string) => string | undefined;
  drawerOpen: boolean;
  actionProps: BreakpointActionPanelProps;
  onBodyChange: (value: string) => void;
  onFormat: () => void;
  onRestore: () => void;
  onValidate: () => void;
  onDrawerChange: (open: boolean) => void;
  onResolve: (kind: BreakpointDecision["kind"]) => void;
}

function MessageTabs({
  label,
  message,
  body,
  headers,
  bytes,
  editable,
  error,
  onChange,
}: {
  label: string;
  message: BreakpointDetailViewModel["original"];
  body: string;
  headers: Record<string, string[]>;
  bytes: number[];
  editable?: boolean;
  error?: string;
  onChange?: (value: string) => void;
}) {
  return (
    <div>
      <h3 className="mb-2 font-semibold">{label}</h3>
      <Tabs defaultSelectedKey="body">
        <Tabs.ListContainer>
          <Tabs.List aria-label={`${label}查看`}>
            <Tabs.Tab id="body">
              {messageContentLabel(message)}
              <Tabs.Indicator />
            </Tabs.Tab>
            <Tabs.Tab id="headers">
              请求头
              <Tabs.Indicator />
            </Tabs.Tab>
            <Tabs.Tab id="bytes">
              原始字节
              <Tabs.Indicator />
            </Tabs.Tab>
          </Tabs.List>
        </Tabs.ListContainer>
        <Tabs.Panel id="body" className="pt-3">
          <HttpBodyViewer
            label={`${label} Body`}
            message={message}
            emptyText="无正文"
            textOverride={body}
            editable={editable}
            error={error}
            ariaLabel={`${label === "有效报文" ? "有效" : "原始"} ${messageContentLabel(message)}`}
            showRawBytes={false}
            onChange={onChange}
          />
        </Tabs.Panel>
        <Tabs.Panel id="headers" className="pt-3">
          <TextArea
            aria-label={`${label}请求头`}
            className="min-h-[430px] font-mono text-xs"
            value={JSON.stringify(headers, null, 2)}
            readOnly
          />
        </Tabs.Panel>
        <Tabs.Panel id="bytes" className="pt-3">
          <TextArea
            aria-label={`${label}原始字节`}
            className="min-h-[430px] font-mono text-xs"
            value={bytes.join(" ")}
            readOnly
          />
        </Tabs.Panel>
      </Tabs>
    </div>
  );
}

export function BreakpointEditorPanel(props: BreakpointEditorPanelProps) {
  const data = props.detail.data;
  if (props.hasSelection && props.detail.error)
    return (
      <div className="min-w-0 overflow-auto p-5">
        <Alert status="danger">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>断点详情读取失败</Alert.Title>
            <Alert.Description>{props.detail.error}</Alert.Description>
          </Alert.Content>
          <Button
            size="sm"
            variant="outline"
            onPress={() => void props.detail.refresh()}
          >
            重试
          </Button>
        </Alert>
      </div>
    );
  if (props.hasSelection && props.detail.isLoading)
    return (
      <div className="grid h-full place-items-center">
        <Spinner aria-label="正在读取断点详情" />
      </div>
    );
  if (!data)
    return (
      <div className="grid h-full place-items-center text-sm text-[var(--telemetry-muted)]">
        选择一条待处理断点
      </div>
    );
  return (
    <div className="min-w-0 overflow-auto p-5">
      <div className="space-y-4">
        <div className="flex min-w-0 flex-wrap items-center gap-x-5 gap-y-2">
          <h2 className="min-w-0 text-lg font-semibold">
            {data.summary.title}
          </h2>
          <span>终端 IP {data.summary.terminal_ip}</span>
          <span>{data.summary.channel_text}通道</span>
          <span className="ml-auto max-w-full truncate font-mono text-xs">
            请求 ID {data.summary.session_id}
          </span>
        </div>
        <div className="grid grid-cols-2 gap-5 max-[1100px]:grid-cols-1">
          <div className="col-span-2">
            <HttpRequestTargetView
              method={data.summary.method}
              target={data.summary.target}
              queryString={(
                data.summary as BreakpointDetailViewModel["summary"] & {
                  query_string?: string | null;
                }
              ).query_string}
            />
          </div>
          <MessageTabs
            label="原始报文"
            message={data.original}
            body={data.original.body_text ?? ""}
            headers={data.original.headers}
            bytes={data.original.body_bytes}
          />
          <MessageTabs
            label="有效报文"
            message={data.effective}
            body={props.bodyText}
            headers={data.effective.headers}
            bytes={data.effective.body_bytes}
            editable
            error={
              props.validationError("message.body_text") ??
              props.validationError("message")
            }
            onChange={props.onBodyChange}
          />
        </div>
        {props.validationError("message.headers") && (
          <p className="text-sm text-danger">
            {props.validationError("message.headers")}
          </p>
        )}
        <div className="flex flex-wrap gap-3">
          <Button
            variant="outline"
            isDisabled={Boolean(props.editorPending) || props.resolvePending}
            onPress={props.onFormat}
          >
            {props.editorPending === "format" ? "正在格式化…" : "格式化 JSON"}
          </Button>
          <Button
            variant="outline"
            onPress={() => void navigator.clipboard.writeText(props.bodyText)}
          >
            <Copy className="size-4" />
            复制
          </Button>
          <Button
            variant="outline"
            isDisabled={Boolean(props.editorPending) || props.resolvePending}
            onPress={props.onRestore}
          >
            {props.editorPending === "restore" ? "正在恢复…" : "恢复原始报文"}
          </Button>
          <Button
            className="ml-auto max-[1280px]:ml-0"
            variant="outline"
            isDisabled={Boolean(props.editorPending) || props.resolvePending}
            onPress={props.onValidate}
          >
            {props.editorPending === "validate" ? "正在校验…" : "由 Rust 校验"}
          </Button>
          <Drawer isOpen={props.drawerOpen} onOpenChange={props.onDrawerChange}>
            <Button
              className="hidden max-[1280px]:inline-flex"
              variant="outline"
            >
              处理断点
            </Button>
            <Drawer.Backdrop isDismissable={!props.resolvePending}>
              <Drawer.Content placement="right">
                <Drawer.Dialog>
                  <Drawer.Header>
                    <Drawer.Heading>处理断点</Drawer.Heading>
                  </Drawer.Header>
                  <Drawer.Body>
                    <BreakpointActionPanel {...props.actionProps} compact />
                  </Drawer.Body>
                  <Drawer.Footer>
                    <Button
                      slot="close"
                      variant="outline"
                      isDisabled={props.resolvePending}
                    >
                      取消
                    </Button>
                    <Button
                      variant="primary"
                      isDisabled={
                        props.resolvePending ||
                        !data.can_resolve ||
                        props.validation?.valid === false
                      }
                      onPress={() =>
                        props.actionProps.selected &&
                        props.onResolve(props.actionProps.selected.kind)
                      }
                    >
                      {props.resolvePending ? "正在处理…" : "执行所选处理"}
                    </Button>
                  </Drawer.Footer>
                </Drawer.Dialog>
              </Drawer.Content>
            </Drawer.Backdrop>
          </Drawer>
        </div>
        {props.validation && (
          <Alert status={props.validation.valid ? "success" : "danger"}>
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>
                {props.validation.valid ? "报文校验通过" : "报文校验失败"}
              </Alert.Title>
              <Alert.Description>
                {props.validation.valid
                  ? props.validation.warnings.join("；") ||
                    "JSON、Shift-JIS 和报文长度有效。"
                  : Object.values(props.validation.field_errors)
                      .flat()
                      .join("；")}
              </Alert.Description>
            </Alert.Content>
          </Alert>
        )}
      </div>
    </div>
  );
}
