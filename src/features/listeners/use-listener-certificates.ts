import { useState, type Dispatch, type SetStateAction } from "react";
import { toast } from "@heroui/react";
import type {
  ListenerCertificateDetailViewModel,
  ListenerCertificateImportViewModel,
  ProxyListener,
  ProxyWorkspace,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand } from "@/lib/ipc/client";
import { mergeCertificateDetails, pruneDetachedDraftCertificates } from "./listener-workspace-draft";
import type { ListenerPending } from "./listener-runtime-card";
import { useDraftCertificateLeases } from "./use-draft-certificate-leases";
import { socketDownstreamTls, socketUpstreamTls } from "./listener-data-plane";

type ImportPending = Extract<ListenerPending, `import-${string}`>;
type RunPending = (
  kind: ListenerPending,
  action: () => Promise<void>,
  onError?: (reason: unknown) => void,
) => Promise<void>;

export function useListenerCertificates({
  currentId,
  workspace,
  selected,
  selectedIndex,
  pending,
  persistedReferences,
  setWorkspace,
  clearDerivedResults,
  runPending,
}: {
  currentId?: string;
  workspace?: ProxyWorkspace;
  selected?: ProxyListener;
  selectedIndex: number;
  pending?: ListenerPending;
  persistedReferences: ProxyWorkspace["certificate_references"];
  setWorkspace: Dispatch<SetStateAction<ProxyWorkspace | undefined>>;
  clearDerivedResults: () => void;
  runPending: RunPending;
}) {
  const leases = useDraftCertificateLeases(currentId);
  const [importedDetails, setImportedDetails] = useState<ListenerCertificateDetailViewModel[]>([]);

  function applyDraftWorkspace(next: ProxyWorkspace, previous: ProxyWorkspace) {
    const { workspace: pruned, detached } = pruneDetachedDraftCertificates(
      previous,
      next,
      persistedReferences,
    );
    setWorkspace(pruned);
    if (detached.length === 0) return;
    const detachedIds = new Set(detached.map((reference) => reference.id));
    setImportedDetails((current) => current.filter((detail) => !detachedIds.has(detail.reference_id)));
    for (const reference of detached) leases.discard(reference);
  }

  async function importCertificate(
    kind: ImportPending,
    load: () => Promise<ListenerCertificateImportViewModel | null>,
    bind: (listener: ProxyListener, referenceId: string) => ProxyListener,
  ) {
    if (!workspace || !selected || pending) return false;
    let importedSuccessfully = false;
    await runPending(kind, async () => {
      const result = await load();
      if (!result) return;
      const { reference, detail } = result;
      if (!leases.claim(workspace.id, reference)) return;
      applyDraftWorkspace({
        ...workspace,
        listeners: workspace.listeners.map((listener, index) =>
          index === selectedIndex ? bind(listener, reference.id) : listener),
        certificate_references: [
          ...workspace.certificate_references.filter((item) => item.id !== reference.id),
          reference,
        ],
      }, workspace);
      clearDerivedResults();
      setImportedDetails((current) => mergeCertificateDetails(current, [detail]));
      importedSuccessfully = true;
      toast("证书材料已安全导入并绑定到当前监听。", { variant: "success" });
    });
    return importedSuccessfully;
  }

  return {
    leases,
    importedDetails,
    applyDraftWorkspace,
    importDownstreamIdentity: (label: string) => importCertificate(
      "import-downstream-identity",
      () => callCommand(commands.listenerImportDownstreamServerIdentity(label)),
      (listener, referenceId) => bindDownstreamIdentity(listener, referenceId),
    ),
    importDownstreamTrust: (label: string) => importCertificate(
      "import-downstream-trust",
      () => callCommand(commands.listenerImportDownstreamClientTrust(label)),
      (listener, referenceId) => bindDownstreamTrust(listener, referenceId),
    ),
    importUpstreamIdentity: (label: string, password: string) => importCertificate(
      "import-upstream-identity",
      () => callCommand(commands.listenerImportUpstreamClientIdentity(label, password)),
      (listener, referenceId) => bindUpstream(listener, { client_identity: referenceId }),
    ),
    importUpstreamTrust: (label: string) => importCertificate(
      "import-upstream-trust",
      () => callCommand(commands.listenerImportUpstreamServerTrust(label)),
      (listener, referenceId) => bindUpstream(listener, { server_trust: referenceId }),
    ),
  };
}

function bindDownstreamIdentity(listener: ProxyListener, referenceId: string): ProxyListener {
  if (listener.data_plane.kind === "http") {
    const settings = listener.data_plane.settings;
    return { ...listener, data_plane: { kind: "http", settings: {
      ...settings,
      downstream_tls: { ...settings.downstream_tls, server_identity: referenceId },
    } } };
  }
  const settings = listener.data_plane.settings;
  const topology = settings.topology;
  if (topology.mode === "local_responder") {
    const downstream = topology.settings.downstream_security;
    if (downstream.mode !== "tls") return listener;
    return { ...listener, data_plane: { kind: "socket", settings: {
      ...settings,
      topology: { mode: "local_responder", settings: {
        downstream_security: {
          ...downstream,
          downstream_tls: { ...downstream.downstream_tls, server_identity: referenceId },
        },
      } },
    } } };
  }
  const currentSecurity = topology.settings.security;
  const tls = socketDownstreamTls(currentSecurity);
  if (!tls) return listener;
  const downstream_tls = { ...tls, server_identity: referenceId };
  const security = currentSecurity.mode === "tls_to_tls"
    ? { ...currentSecurity, downstream_tls }
    : { ...currentSecurity, downstream_tls };
  return { ...listener, data_plane: { kind: "socket", settings: {
    ...settings,
    topology: { mode: "relay", settings: { ...topology.settings, security } },
  } } };
}

function bindDownstreamTrust(listener: ProxyListener, referenceId: string): ProxyListener {
  if (listener.data_plane.kind === "http") {
    const settings = listener.data_plane.settings;
    const current = settings.downstream_tls.client_authentication;
    const client_authentication = current.mode === "required"
      ? { mode: "required" as const, trust: referenceId }
      : { mode: "optional" as const, trust: referenceId };
    return { ...listener, data_plane: { kind: "http", settings: {
      ...settings,
      downstream_tls: { ...settings.downstream_tls, client_authentication },
    } } };
  }
  const settings = listener.data_plane.settings;
  const topology = settings.topology;
  if (topology.mode === "local_responder") {
    const downstream = topology.settings.downstream_security;
    if (downstream.mode !== "tls") return listener;
    const tls = downstream.downstream_tls;
    const required = tls.client_authentication.mode === "required";
    return { ...listener, data_plane: { kind: "socket", settings: {
      ...settings,
      topology: { mode: "local_responder", settings: {
        downstream_security: { ...downstream, downstream_tls: {
          ...tls,
          client_authentication: required
            ? { mode: "required", trust: referenceId }
            : { mode: "optional", trust: referenceId },
        } },
      } },
    } } };
  }
  const currentSecurity = topology.settings.security;
  const tls = socketDownstreamTls(currentSecurity);
  if (!tls) return listener;
  const required = tls.client_authentication.mode === "required";
  const downstream_tls = {
    ...tls,
    client_authentication: required
      ? { mode: "required" as const, trust: referenceId }
      : { mode: "optional" as const, trust: referenceId },
  };
  const security = currentSecurity.mode === "tls_to_tls"
    ? { ...currentSecurity, downstream_tls }
    : { ...currentSecurity, downstream_tls };
  return { ...listener, data_plane: { kind: "socket", settings: {
    ...settings,
    topology: { mode: "relay", settings: { ...topology.settings, security } },
  } } };
}

function bindUpstream(
  listener: ProxyListener,
  changes: { client_identity?: string; server_trust?: string },
): ProxyListener {
  if (listener.data_plane.kind === "http") {
    const settings = listener.data_plane.settings;
    const fixed = settings.fixed_server;
    if (!fixed) return listener;
    return { ...listener, data_plane: { kind: "http", settings: {
      ...settings,
      fixed_server: { ...fixed, upstream_tls: { ...fixed.upstream_tls, ...changes } },
    } } };
  }
  const settings = listener.data_plane.settings;
  const topology = settings.topology;
  if (topology.mode === "local_responder") return listener;
  const currentSecurity = topology.settings.security;
  const tls = socketUpstreamTls(currentSecurity);
  if (!tls) return listener;
  const upstream_tls = { ...tls, ...changes };
  const security = currentSecurity.mode === "tls_to_tls"
    ? { ...currentSecurity, upstream_tls }
    : { ...currentSecurity, upstream_tls };
  return { ...listener, data_plane: { kind: "socket", settings: {
    ...settings,
    topology: { mode: "relay", settings: { ...topology.settings, security } },
  } } };
}
