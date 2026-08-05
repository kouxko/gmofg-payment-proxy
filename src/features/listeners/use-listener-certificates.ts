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
      (listener, referenceId) => ({
        ...listener,
        downstream_tls: { ...listener.downstream_tls, server_identity: referenceId },
      }),
    ),
    importDownstreamTrust: (label: string) => importCertificate(
      "import-downstream-trust",
      () => callCommand(commands.listenerImportDownstreamClientTrust(label)),
      (listener, referenceId) => ({
        ...listener,
        downstream_tls: {
          ...listener.downstream_tls,
          client_authentication: listener.downstream_tls.client_authentication.mode === "required"
            ? { mode: "required", trust: referenceId }
            : { mode: "optional", trust: referenceId },
        },
      }),
    ),
    importUpstreamIdentity: (label: string, password: string) => importCertificate(
      "import-upstream-identity",
      () => callCommand(commands.listenerImportUpstreamClientIdentity(label, password)),
      (listener, referenceId) => listener.fixed_server ? {
        ...listener,
        fixed_server: {
          ...listener.fixed_server,
          upstream_tls: { ...listener.fixed_server.upstream_tls, client_identity: referenceId },
        },
      } : listener,
    ),
    importUpstreamTrust: (label: string) => importCertificate(
      "import-upstream-trust",
      () => callCommand(commands.listenerImportUpstreamServerTrust(label)),
      (listener, referenceId) => listener.fixed_server ? {
        ...listener,
        fixed_server: {
          ...listener.fixed_server,
          upstream_tls: { ...listener.fixed_server.upstream_tls, server_trust: referenceId },
        },
      } : listener,
    ),
  };
}
