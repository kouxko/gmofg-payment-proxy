"use client";

import { useEffect, useRef } from "react";
import { Alert, Button, Chip, Modal, Spinner } from "@heroui/react";
import { schemaNodeCount, schemaTitle } from "@/lib/protocol-package-schema";
import { capabilityItems, protocolPackageKindText } from "./protocol-package-model";
import type {
  ImportPreviewDisplay,
  ProtocolPackageImportState,
} from "./protocol-package-import-model";

export function ProtocolPackageImportDialog({
  state,
  onOpenChange,
  onChoose,
  onCommit,
  onRefresh,
}: {
  state: ProtocolPackageImportState;
  onOpenChange: (open: boolean) => void;
  onChoose: () => void;
  onCommit: () => void;
  onRefresh: () => void;
}) {
  const busy = ["preparing", "committing", "refreshing", "discarding"].includes(state.kind);
  const error = state.kind === "prepare-error"
    || state.kind === "commit-error"
    || state.kind === "refresh-error"
    || state.kind === "discard-error" ? state.error : undefined;
  const errorRef = useRef<HTMLDivElement | null>(null);
  const previewHeadingRef = useRef<HTMLHeadingElement | null>(null);
  useEffect(() => {
    // Modal 首次打开时会先聚焦 Dialog；错误随后异步抵达时再显式移动焦点，
    // 让键盘和读屏用户立即落到错误码、文件及行列信息上。
    if (error) errorRef.current?.focus();
  }, [error]);
  useEffect(() => {
    if (state.kind === "ready" || state.kind === "conflict") previewHeadingRef.current?.focus();
  }, [state]);
  return (
    <Modal isOpen={state.kind !== "closed"} onOpenChange={onOpenChange}>
      <Button className="hidden" aria-hidden="true">打开协议包 ZIP 导入</Button>
      <Modal.Backdrop isDismissable={!busy}>
        <Modal.Container size="lg" scroll="inside">
          <Modal.Dialog>
            <Modal.Header>
              <Modal.Heading>导入协议包 ZIP</Modal.Heading>
            </Modal.Header>
            <Modal.Body className="min-h-40 space-y-5" aria-busy={busy}>
              {state.kind === "preparing" && (
                <div className="grid min-h-32 place-items-center gap-3 text-center" aria-live="polite">
                  <Spinner aria-label="正在选择并完整校验协议包 ZIP" />
                  <p className="text-sm text-[var(--telemetry-muted)]">正在读取 ZIP，并校验 Manifest、Schema、JavaScript 模块与入口……</p>
                </div>
              )}
              {state.kind === "committing" && <BusyMessage label="正在安装协议包" detail="一次性确认凭据已提交，正在等待应用核心完成原子安装。" />}
              {state.kind === "refreshing" && <BusyMessage label="正在刷新协议包列表" detail="协议包已经安装，正在定位对应精确版本。" />}
              {state.kind === "discarding" && <BusyMessage label="正在释放导入预览" detail="正在安全释放尚未提交的一次性确认凭据。" />}
              {!busy && error && (
                <Alert ref={errorRef} status="danger" role="alert" tabIndex={-1}>
                  <Alert.Indicator />
                  <Alert.Content>
                    <Alert.Title>
                      {state.kind === "refresh-error"
                        ? "协议包已安装，但列表刷新失败"
                        : state.kind === "discard-error" ? "导入预览释放失败" : "协议包导入失败"}
                      {error.code ? `（${error.code}）` : ""}
                    </Alert.Title>
                    <Alert.Description>
                      <span className="block break-words">{error.message}</span>
                      {error.details.length > 0 && (
                        <ul className="mt-2 list-disc space-y-1 pl-5">
                          {error.details.map((detail) => <li key={detail} className="break-all font-mono text-xs">{detail}</li>)}
                        </ul>
                      )}
                      {state.kind === "prepare-error" && <span className="mt-2 block text-sm">未安装任何协议包内容。</span>}
                    </Alert.Description>
                  </Alert.Content>
                </Alert>
              )}
              {(state.kind === "ready" || state.kind === "committing") && (
                <ImportPreview preview={state.preview} headingRef={previewHeadingRef} />
              )}
              {state.kind === "conflict" && <ImportPreview preview={state.preview} headingRef={previewHeadingRef} />}
              {state.kind === "closed" && (
                <p className="text-sm text-[var(--telemetry-muted)]">选择 ZIP 后，应用核心会先完成全部校验，再显示不含源码的预览。</p>
              )}
            </Modal.Body>
            <Modal.Footer className="shrink-0 flex-wrap border-t border-[var(--telemetry-line)] pt-4">
              <Button slot="close" variant="outline" isDisabled={busy}>取消</Button>
              {(state.kind === "prepare-error" || state.kind === "commit-error" || state.kind === "conflict") && (
                <Button variant="outline" isDisabled={busy} onPress={onChoose}>重新选择 ZIP</Button>
              )}
              {state.kind === "discard-error" && <Button variant="primary" onPress={() => onOpenChange(false)}>重试释放并关闭</Button>}
              {state.kind === "refresh-error" && (
                <Button variant="primary" onPress={onRefresh}>重试刷新列表</Button>
              )}
              {state.kind === "ready" && (
                <Button variant="primary" isDisabled={busy} onPress={onCommit}>
                  确认安装
                </Button>
              )}
            </Modal.Footer>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}

function BusyMessage({ label, detail }: { label: string; detail: string }) {
  return (
    <div className="grid min-h-32 place-items-center gap-3 text-center" aria-live="polite">
      <Spinner aria-label={label} />
      <p className="text-sm text-[var(--telemetry-muted)]">{detail}</p>
    </div>
  );
}

function ImportPreview({
  preview,
  headingRef,
}: {
  preview: ImportPreviewDisplay;
  headingRef: React.RefObject<HTMLHeadingElement | null>;
}) {
  return (
    <div className="space-y-5" aria-label="协议包无源码预览">
      <section aria-labelledby="import-identity-heading">
        <h3 ref={headingRef} tabIndex={-1} id="import-identity-heading" className="mb-2 font-semibold">身份预览</h3>
        <dl className="grid grid-cols-[7rem_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm max-[560px]:grid-cols-1">
          <dt className="text-[var(--telemetry-muted)]">名称</dt><dd className="break-words">{preview.name}</dd>
          <dt className="text-[var(--telemetry-muted)]">包 ID</dt><dd className="break-all font-mono">{preview.package.id}</dd>
          <dt className="text-[var(--telemetry-muted)]">精确版本</dt><dd className="break-all font-mono">{preview.package.version}</dd>
          <dt className="text-[var(--telemetry-muted)]">Host API</dt><dd>{preview.host_api}</dd>
          <dt className="text-[var(--telemetry-muted)]">适用协议</dt><dd>{protocolPackageKindText(preview.kind)}</dd>
          <dt className="text-[var(--telemetry-muted)]">上行 Schema</dt>
          <dd className="break-words">
            {preview.upstream_schema
              ? `${schemaTitle(preview.upstream_schema)} · ${schemaNodeCount(preview.upstream_schema.root)} 个节点`
              : "无 Schema"}
          </dd>
          <dt className="text-[var(--telemetry-muted)]">下行 Schema</dt>
          <dd className="break-words">
            {preview.downstream_schema
              ? `${schemaTitle(preview.downstream_schema)} · ${schemaNodeCount(preview.downstream_schema.root)} 个节点`
              : "无 Schema"}
          </dd>
        </dl>
      </section>
      <section aria-labelledby="import-capabilities-heading">
        <h3 id="import-capabilities-heading" className="mb-2 font-semibold">能力</h3>
        <div className="flex flex-wrap gap-2">
          {capabilityItems(preview.capabilities).map(([label, supported]) => (
            <Chip key={label} size="sm" color={supported ? "success" : "default"} variant="soft">
              {label}：{supported ? "支持" : "不支持"}
            </Chip>
          ))}
        </div>
      </section>
      <DispositionNotice disposition={preview.disposition} />
      <p className="text-xs text-[var(--telemetry-muted)]">预览仅包含声明与校验结果，不会把 ZIP、脚本源码、本机文件路径或 AST 发送到 WebView。</p>
      <p className="text-xs text-[var(--telemetry-muted)]">新安装的协议包默认停用，不会修改或重绑任何入口。</p>
    </div>
  );
}

function DispositionNotice({ disposition }: { disposition: ImportPreviewDisplay["disposition"] }) {
  if (disposition === "identity_conflict") {
    return <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>精确身份内容冲突</Alert.Title><Alert.Description>相同包 ID 与版本已存在，但内容不同。请修改协议包版本后重新选择；此预览不能提交。</Alert.Description></Alert.Content></Alert>;
  }
  return (
    <Alert status={disposition === "reusable" ? "warning" : "success"}>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{disposition === "reusable" ? "可复用精确版本" : "可安装新版本"}</Alert.Title>
        <Alert.Description>{disposition === "reusable" ? "相同身份与内容已存在；确认后将幂等复用，不会覆盖。" : "当前注册表快照中没有此精确身份。"}</Alert.Description>
      </Alert.Content>
    </Alert>
  );
}
