"use client";

import { useCallback, useEffect, useRef } from "react";
import { toast } from "@heroui/react";
import type { CertificateReference } from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";

interface DraftCertificateLease {
  reference: CertificateReference;
  workspaceId: string;
  committing: boolean;
  cleanupRequested: boolean;
}

/**
 * Owns imported certificate material until Rust confirms that its reference was saved.
 * Draft material is discarded when it is detached, its workspace changes, or the page
 * unmounts. A save already in flight resolves first so cleanup cannot race the commit.
 */
export function useDraftCertificateLeases(workspaceId?: string) {
  const leases = useRef(new Map<string, DraftCertificateLease>());
  const discardsInFlight = useRef(new Set<string>());
  const activeWorkspaceId = useRef(workspaceId);

  const discardNow = useCallback((reference: CertificateReference, reportError: boolean) => {
    if (discardsInFlight.current.has(reference.reference)) return;
    discardsInFlight.current.add(reference.reference);
    void callCommand(commands.listenerCertificateDiscard(reference))
      .catch((reason) => {
        if (reportError) toast(errorMessage(reason), { variant: "danger" });
      })
      .finally(() => discardsInFlight.current.delete(reference.reference));
  }, []);

  const discard = useCallback((reference: CertificateReference, reportError = true) => {
    const lease = leases.current.get(reference.reference);
    if (lease?.committing) {
      lease.cleanupRequested = true;
      return;
    }
    leases.current.delete(reference.reference);
    discardNow(reference, reportError);
  }, [discardNow]);

  const claim = useCallback((claimWorkspaceId: string, reference: CertificateReference) => {
    if (activeWorkspaceId.current !== claimWorkspaceId) {
      discardNow(reference, false);
      return false;
    }
    leases.current.set(reference.reference, {
      reference,
      workspaceId: claimWorkspaceId,
      committing: false,
      cleanupRequested: false,
    });
    return true;
  }, [discardNow]);

  const beginCommit = useCallback((references: CertificateReference[]) => {
    const handles = references.flatMap((reference) => {
      const lease = leases.current.get(reference.reference);
      if (!lease) return [];
      lease.committing = true;
      return [reference.reference];
    });

    return (persistedReferences?: CertificateReference[]) => {
      const persistedIds = new Set(persistedReferences?.map((reference) => reference.id));
      const persistedHandles = new Set(
        persistedReferences?.map((reference) => reference.reference),
      );
      for (const handle of handles) {
        const lease = leases.current.get(handle);
        if (!lease) continue;
        if (persistedHandles.has(handle) || persistedIds.has(lease.reference.id)) {
          leases.current.delete(handle);
          continue;
        }
        lease.committing = false;
        if (lease.cleanupRequested) discard(lease.reference, false);
      }
    };
  }, [discard]);

  useEffect(() => {
    activeWorkspaceId.current = workspaceId;
    if (!workspaceId) return;
    const leaseMap = leases.current;
    return () => {
      if (activeWorkspaceId.current === workspaceId) activeWorkspaceId.current = undefined;
      for (const lease of [...leaseMap.values()]) {
        if (lease.workspaceId === workspaceId) discard(lease.reference, false);
      }
    };
  }, [discard, workspaceId]);

  return { beginCommit, claim, discard };
}
