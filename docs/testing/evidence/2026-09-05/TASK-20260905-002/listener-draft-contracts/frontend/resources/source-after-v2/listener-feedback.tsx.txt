import { Alert, Button } from "@heroui/react";
import type { ListenerDraft, ListenerDraftValidationViewModel } from "@/generated/rust-types";
import { isLocalResponder } from "./listener-view-model";

export interface ListenerOperationError {
  message: string;
  fieldErrors: Record<string, string[]>;
}

export function ListenerFeedback({ listener, validation, operationError, conflict, pending, onDiscardConflict }: {
  listener?: ListenerDraft;
  validation?: ListenerDraftValidationViewModel;
  operationError?: ListenerOperationError;
  conflict: boolean;
  pending: boolean;
  onDiscardConflict: () => void;
}) {
  const errors = Object.entries(validation?.field_errors ?? {});
  return <>
    {conflict && <Alert status="danger"><Alert.Indicator /><Alert.Content>
      <Alert.Title>当前监听存在外部修改冲突</Alert.Title>
      <Alert.Description>本地草稿已保留。为避免覆盖最新配置，保存、启动和删除已暂停。</Alert.Description>
      <div className="mt-3"><Button size="sm" variant="danger-soft" isDisabled={pending} onPress={onDiscardConflict}>放弃当前监听草稿并加载最新配置</Button></div>
    </Alert.Content></Alert>}
    {validation && (validation.valid ? <Alert status="success"><Alert.Indicator /><Alert.Content><Alert.Title>当前监听校验通过</Alert.Title><Alert.Description>{conflict ? "结构有效，但须先解决外部修改冲突。" : isLocalResponder(listener) ? "配置结构有效，可保存并启动；收到请求后将由本机生成响应。" : "当前监听可保存、启动或执行上游 TLS 测试。"}</Alert.Description></Alert.Content></Alert> : <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>当前监听校验未通过</Alert.Title><Alert.Description>{listener?.data_plane.kind === "socket" ? "请按下方分类修正 Socket 配置。" : errors.map(([field, messages]) => `${field}: ${messages.join("，")}`).join("；")}</Alert.Description></Alert.Content></Alert>)}
    {operationError && <Alert status="danger"><Alert.Indicator /><Alert.Content>
      <Alert.Title>{listener?.data_plane.kind === "socket" ? "操作未完成" : operationError.message}</Alert.Title>
      <Alert.Description>{listener?.data_plane.kind === "socket" ? <>
        <p>请按下方分类修正 Socket 配置。</p>
        <details className="mt-2"><summary className="cursor-pointer font-medium">高级诊断</summary>
          <p className="mt-1">{operationError.message}</p>
          <ul>{Object.entries(operationError.fieldErrors).map(([field, messages]) => <li key={field}>{field}: {messages.join("，")}</li>)}</ul>
        </details>
      </> : Object.entries(operationError.fieldErrors).map(([field, messages]) => `${field}: ${messages.join("，")}`).join("；")}</Alert.Description>
    </Alert.Content></Alert>}
  </>;
}
