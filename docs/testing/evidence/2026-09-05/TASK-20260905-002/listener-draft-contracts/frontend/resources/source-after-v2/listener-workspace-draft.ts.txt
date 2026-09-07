import type {
  CertificateReference,
  ListenerDraft,
  ListenerCertificateDetailViewModel,
  ProxyListener,
  ProxyWorkspace,
} from "@/generated/rust-types";

export type ListenerWorkspaceDraft = Omit<ProxyWorkspace, "listeners"> & {
  listeners: ListenerDraft[];
};

export function toListenerWorkspaceDraft(workspace: ProxyWorkspace): ListenerWorkspaceDraft {
  return workspace;
}

export function sameWorkspace(left: ListenerWorkspaceDraft, right: ProxyWorkspace) {
  return JSON.stringify(left) === JSON.stringify(right);
}

/**
 * Rebase listener-local drafts onto the newest authoritative workspace.
 * A workspace revision can advance because a different listener changed, so conflicts are
 * detected from the listener values themselves instead of the aggregate revision.
 */
export function reconcileListenerDraft(
  draft: ListenerWorkspaceDraft,
  base: ProxyWorkspace,
  latest: ProxyWorkspace,
  existingConflictingListenerIds: readonly string[] = [],
) {
  const baseById = new Map(base.listeners.map((listener) => [listener.id, listener]));
  const latestById = new Map(latest.listeners.map((listener) => [listener.id, listener]));
  const draftIds = new Set(draft.listeners.map((listener) => listener.id));
  const conflicts = new Set(existingConflictingListenerIds);
  const listeners = draft.listeners.flatMap((draftListener) => {
    const baseListener = baseById.get(draftListener.id);
    const latestListener = latestById.get(draftListener.id);
    if (!latestListener) {
      if (!baseListener) return [draftListener];
      if (!sameListener(draftListener, baseListener)) {
        conflicts.add(draftListener.id);
        return [draftListener];
      }
      return [];
    }
    if (!baseListener) {
      if (sameListener(draftListener, latestListener)) {
        conflicts.delete(draftListener.id);
        return [latestListener];
      }
      conflicts.add(latestListener.id);
      return [draftListener];
    }
    const draftChanged = !sameListener(draftListener, baseListener);
    const authorityChanged = !sameListener(latestListener, baseListener);
    if (!draftChanged || sameListener(draftListener, latestListener)) {
      conflicts.delete(draftListener.id);
      return [latestListener];
    }
    if (authorityChanged) conflicts.add(latestListener.id);
    return [draftListener];
  });

  for (const latestListener of latest.listeners) {
    if (!draftIds.has(latestListener.id)) listeners.push(latestListener);
  }

  const reachableIds = listenerCertificateReferenceIds(listeners);
  const references = new Map(
    latest.certificate_references.map((reference) => [reference.id, reference]),
  );
  for (const reference of draft.certificate_references) {
    if (reachableIds.has(reference.id) && !references.has(reference.id)) {
      references.set(reference.id, reference);
    }
  }
  return {
    workspace: {
      ...latest,
      listeners,
      certificate_references: [...references.values()],
    },
    conflictingListenerIds: [...conflicts],
  };
}

function sameListener(left: ListenerDraft | ProxyListener, right: ListenerDraft | ProxyListener) {
  return JSON.stringify(left) === JSON.stringify(right);
}

/**
 * Rust 只持久化当前监听；其他监听仍可能包含用户尚未保存的草稿。
 * 用新的 revision、证书引用和当前监听覆盖本地草稿，避免保存 B 时丢失 A 的输入。
 */
export function mergePersistedListener(
  draft: ListenerWorkspaceDraft,
  persisted: ProxyWorkspace,
  listenerId: string,
) {
  const persistedListener = persisted.listeners.find((listener) => listener.id === listenerId);
  const draftIds = new Set(draft.listeners.map((listener) => listener.id));
  const listeners = draft.listeners.map((listener) =>
    listener.id === listenerId && persistedListener ? persistedListener : listener,
  );
  for (const listener of persisted.listeners) {
    if (!draftIds.has(listener.id)) listeners.push(listener);
  }
  return mergeReachableCertificateReferences(draft, persisted, listeners);
}

/** 保留被删除监听以外的本地草稿与它们尚未保存的托管证书引用。 */
export function mergePersistedListenerDeletion(
  draft: ListenerWorkspaceDraft,
  persisted: ProxyWorkspace,
  deletedListenerId: string,
) {
  const draftIds = new Set(draft.listeners.map((listener) => listener.id));
  const listeners = draft.listeners.filter((listener) => listener.id !== deletedListenerId);
  for (const listener of persisted.listeners) {
    if (!draftIds.has(listener.id)) listeners.push(listener);
  }
  return mergeReachableCertificateReferences(draft, persisted, listeners);
}

function mergeReachableCertificateReferences(
  draft: ListenerWorkspaceDraft,
  persisted: ProxyWorkspace,
  listeners: ListenerDraft[],
) {
  const reachableIds = listenerCertificateReferenceIds(listeners);
  const references = new Map(
    persisted.certificate_references.map((reference) => [reference.id, reference]),
  );
  for (const reference of draft.certificate_references) {
    if (reachableIds.has(reference.id) && !references.has(reference.id)) {
      references.set(reference.id, reference);
    }
  }
  return {
    ...draft,
    revision: persisted.revision,
    listeners,
    certificate_references: [...references.values()],
  };
}

export function mergeCertificateDetails(
  first: ListenerCertificateDetailViewModel[],
  second: ListenerCertificateDetailViewModel[],
) {
  const details = new Map(first.map((detail) => [detail.reference_id, detail]));
  for (const detail of second) details.set(detail.reference_id, detail);
  return [...details.values()];
}

/** Listener 级命令只携带当前监听实际可达的安全引用。 */
export function listenerCertificateReferences(
  listener: ListenerDraft | ProxyListener,
  references: ProxyWorkspace["certificate_references"],
) {
  const referencedIds = listenerCertificateReferenceIds([listener]);
  return references.filter((reference) => referencedIds.has(reference.id));
}

function listenerCertificateReferenceIds(listeners: readonly (ListenerDraft | ProxyListener)[]) {
  const referencedIds = new Set<string>();
  for (const listener of listeners) {
    if (listener.data_plane.kind === "http") {
      collectDownstream(
        listener.data_plane.settings.downstream_tls.server_identity,
        listener.data_plane.settings.downstream_tls.client_authentication,
        referencedIds,
      );
      const topology = listener.data_plane.settings.topology;
      if (topology.mode === "remote_server") {
        collectUpstream(topology.settings.fixed_server?.upstream_tls, referencedIds);
      }
      continue;
    }
    const topology = listener.data_plane.settings.topology;
    if (topology.mode === "local_responder") {
      if (topology.settings.downstream_security.mode === "tls") {
        const tls = topology.settings.downstream_security.downstream_tls;
        collectDownstream(tls.server_identity, tls.client_authentication, referencedIds);
      }
      continue;
    }
    const security = topology.settings.security;
    if (security.mode === "tls_to_tcp" || security.mode === "tls_to_tls") {
      collectDownstream(
        security.downstream_tls.server_identity,
        security.downstream_tls.client_authentication,
        referencedIds,
      );
    }
    if (security.mode === "tcp_to_tls" || security.mode === "tls_to_tls") {
      collectUpstream(security.upstream_tls, referencedIds);
    }
  }
  return referencedIds;
}

function collectDownstream(
  serverIdentity: string | null,
  authentication: { mode: "disabled" } | { mode: "optional" | "required"; trust: string | null },
  ids: Set<string>,
) {
  if (serverIdentity) ids.add(serverIdentity);
  if (authentication.mode !== "disabled" && authentication.trust) ids.add(authentication.trust);
}

function collectUpstream(
  tls: { server_trust: string | null; client_identity: string | null } | undefined,
  ids: Set<string>,
) {
  if (tls?.server_trust) ids.add(tls.server_trust);
  if (tls?.client_identity) ids.add(tls.client_identity);
}

/**
 * 导入命令先把证书写入系统安全存储。若草稿不再引用且尚未持久化，则返回待清理引用。
 */
export function pruneDetachedDraftCertificates(
  previous: ListenerWorkspaceDraft,
  next: ListenerWorkspaceDraft,
  persistedReferences: CertificateReference[],
) {
  const reachableIds = listenerCertificateReferenceIds(next.listeners);
  const persistedIds = new Set(persistedReferences.map((reference) => reference.id));
  const persistedHandles = new Set(persistedReferences.map((reference) => reference.reference));
  const detached = previous.certificate_references.filter((reference) =>
    !reachableIds.has(reference.id)
    && !persistedIds.has(reference.id)
    && !persistedHandles.has(reference.reference),
  );
  if (detached.length === 0) return { workspace: next, detached };
  const detachedIds = new Set(detached.map((reference) => reference.id));
  return {
    workspace: {
      ...next,
      certificate_references: next.certificate_references.filter(
        (reference) => !detachedIds.has(reference.id),
      ),
    },
    detached,
  };
}
