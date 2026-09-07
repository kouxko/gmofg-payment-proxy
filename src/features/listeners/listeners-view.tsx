"use client";

import { Alert, Button, Chip } from "@heroui/react";
import { ListenerEditor } from "./listener-editor";
import { ListenerFeedback } from "./listener-feedback";
import { ListenerListPanel } from "./listener-list-panel";
import { ListenerRuntimeCard } from "./listener-runtime-card";
import { listenerKind } from "./listener-view-model";
import { useListenerController } from "./use-listener-controller";

export function ListenersView() {
  const state = useListenerController();
  const {
    effectiveWorkspace, effectiveIndex, selected, pending, selectedCanDelete,
    snapshotLocked, selectedHasDraftConflict, draftTransitions,
    persistenceActions, validation, operationError, certificateDetails,
    selectedStatus, listenerOverview, protocolCatalog, editorLocked,
    editorFieldErrors, effectiveCertificateDetails, installationRoot,
    tlsTest, tlsTestError, basicUsername, basicPassword, certificateActions,
  } = state;
  return (
    <section className="grid h-full grid-cols-[420px_minmax(0,1fr)] max-[900px]:grid-cols-1">
      <ListenerListPanel
        listeners={effectiveWorkspace?.listeners ?? []}
        selectedIndex={effectiveIndex}
        loading={state.workspaceQuery.isLoading}
        error={state.workspaceQuery.error}
        disabled={!effectiveWorkspace || Boolean(pending)}
        onAdd={() => void state.addListener()}
        onSelect={state.selectListener}
        onNavigate={state.navigate}
      />
      <main className="min-w-0 space-y-5 overflow-auto p-5">
        <div className="flex flex-wrap items-center gap-3">
          <h2 className="text-xl font-semibold">监听配置</h2>
          {selected && <Chip color="accent" variant="soft">{listenerKind(selected)}</Chip>}
          <div className="ml-auto flex flex-wrap gap-2">
            <Button variant="outline" isDisabled={!selected || Boolean(pending)} onPress={() => void state.copySelectedListener()}>复制监听</Button>
            <Button variant="danger-soft" isDisabled={!selected || !selectedCanDelete || snapshotLocked || Boolean(pending)} onPress={() => void state.removeSelectedListener()}>{pending === "delete" ? "删除中…" : "删除监听"}</Button>
            <Button variant="outline" isDisabled={!selected || Boolean(pending) || draftTransitions.transitioning} onPress={() => void persistenceActions.validate()}>{pending === "validate" ? "校验中…" : "校验当前监听"}</Button>
            <Button variant="primary" isDisabled={!selected || snapshotLocked || selectedHasDraftConflict || Boolean(pending) || draftTransitions.transitioning} onPress={() => void persistenceActions.save()}>{pending === "save" ? "保存中…" : "保存当前监听"}</Button>
          </div>
        </div>
        {draftTransitions.transitioning && <p role="status" className="text-sm text-[var(--telemetry-muted)]">正在切换模式…</p>}
        <ListenerFeedback listener={selected} validation={validation} operationError={operationError} conflict={selectedHasDraftConflict} pending={Boolean(pending)} onDiscardConflict={state.discardSelectedConflictingDraft} />
        {certificateDetails.error && <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>证书详情读取失败</Alert.Title><Alert.Description>{certificateDetails.error}</Alert.Description></Alert.Content></Alert>}
        {!selected ? <p className="py-12 text-center text-sm text-[var(--telemetry-muted)]">选择一个代理监听进行编辑。</p> : <>
          <ListenerRuntimeCard status={selectedStatus} isLoading={listenerOverview.isLoading} error={listenerOverview.error} pending={pending} startBlocked={selectedHasDraftConflict || draftTransitions.transitioning} onToggle={persistenceActions.toggleRuntime} onRetry={listenerOverview.refresh} />
          <ListenerEditor listener={selected} protocolCatalog={{ data: protocolCatalog.data, error: protocolCatalog.error, loading: protocolCatalog.isLoading, refresh: protocolCatalog.refresh }} locked={editorLocked} transitionPending={draftTransitions.transitioning} dataPlanePending={draftTransitions.dataPlanePending} serverControlsBlocked={draftTransitions.serverControlsBlocked} fieldErrors={editorFieldErrors} certificateReferences={effectiveWorkspace?.certificate_references ?? []} certificateDetails={effectiveCertificateDetails} installationRoot={installationRoot} pending={pending} tlsTest={tlsTest} tlsTestError={tlsTestError} basicUsername={basicUsername} basicPassword={basicPassword} onBasicUsernameChange={state.changeBasicUsername} onBasicPasswordChange={state.changeBasicPassword} onChange={state.replaceSelected} onTransition={(transition, recommendedPackage) => draftTransitions.enqueue(transition, recommendedPackage ?? null)} onStoreBasicCredential={state.storeBasicCredential} onImportDownstreamServerIdentity={certificateActions.importDownstreamIdentity} onImportDownstreamClientTrust={certificateActions.importDownstreamTrust} onImportClientIdentity={certificateActions.importUpstreamIdentity} onImportServerTrust={certificateActions.importUpstreamTrust} onTestUpstreamTls={state.testUpstreamTls} />
        </>}
      </main>
    </section>
  );
}
