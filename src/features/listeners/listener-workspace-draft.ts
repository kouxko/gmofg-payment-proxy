import type {
  CertificateReference,
  ListenerCertificateDetailViewModel,
  ProxyListener,
  ProxyWorkspace,
} from "@/generated/rust-types";

export function sameWorkspace(left: ProxyWorkspace, right: ProxyWorkspace) {
  return JSON.stringify(left) === JSON.stringify(right);
}

/**
 * Rust 只持久化当前监听；其他监听仍可能包含用户尚未保存的草稿。
 * 用新的 revision、证书引用和当前监听覆盖本地草稿，避免保存 B 时丢失 A 的输入。
 */
export function mergePersistedListener(
  draft: ProxyWorkspace,
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
  draft: ProxyWorkspace,
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
  draft: ProxyWorkspace,
  persisted: ProxyWorkspace,
  listeners: ProxyListener[],
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
  listener: ProxyListener,
  references: ProxyWorkspace["certificate_references"],
) {
  const referencedIds = listenerCertificateReferenceIds([listener]);
  return references.filter((reference) => referencedIds.has(reference.id));
}

function listenerCertificateReferenceIds(listeners: ProxyListener[]) {
  const referencedIds = new Set<string>();
  for (const listener of listeners) {
    if (listener.data_plane.kind === "http") {
      collectDownstream(
        listener.data_plane.settings.downstream_tls.server_identity,
        listener.data_plane.settings.downstream_tls.client_authentication,
        referencedIds,
      );
      collectUpstream(listener.data_plane.settings.fixed_server?.upstream_tls, referencedIds);
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
  authentication: { mode: "disabled" } | { mode: "optional" | "required"; trust: string },
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
  previous: ProxyWorkspace,
  next: ProxyWorkspace,
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
