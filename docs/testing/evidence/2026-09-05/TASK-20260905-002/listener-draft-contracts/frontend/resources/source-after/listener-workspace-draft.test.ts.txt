import { describe, expect, it } from "vitest";
import type { ProxyWorkspace } from "@/generated/rust-types";
import {
  certificateReference,
  dynamicListener,
  workspace,
} from "./listeners-view.test-support";
import { reconcileListenerDraft } from "./listener-workspace-draft";

describe("reconcileListenerDraft", () => {
  it("refreshes unchanged listeners while preserving a different listener draft", () => {
    const listenerA = dynamicListener("listener-a", "监听 A", 8080);
    const listenerB = dynamicListener("listener-b", "监听 B", 8081);
    const base = {
      ...workspace,
      listeners: [listenerA, listenerB],
      android_network_profiles: [],
    } as ProxyWorkspace;
    const draft = {
      ...base,
      listeners: [{ ...listenerA, name: "监听 A 草稿" }, listenerB],
    };
    const latest = {
      ...base,
      revision: 2,
      listeners: [listenerA, { ...listenerB, name: "监听 B 外部更新" }],
    };

    const result = reconcileListenerDraft(draft, base, latest);

    expect(result.conflictingListenerIds).toEqual([]);
    expect(result.workspace.revision).toBe(2);
    expect(result.workspace.listeners.map((listener) => listener.name)).toEqual(
      [
        "监听 A 草稿",
        "监听 B 外部更新",
      ],
    );
  });

  it("keeps the local draft and reports a conflict when the same listener changed externally", () => {
    const listener = dynamicListener("listener-a", "监听 A", 8080);
    const base = {
      ...workspace,
      listeners: [listener],
      android_network_profiles: [],
    } as ProxyWorkspace;
    const draft = { ...base, listeners: [{ ...listener, name: "本地草稿" }] };
    const latest = {
      ...base,
      revision: 2,
      listeners: [{ ...listener, name: "外部修改" }],
    };

    const result = reconcileListenerDraft(draft, base, latest);

    expect(result.conflictingListenerIds).toEqual(["listener-a"]);
    expect(result.workspace.revision).toBe(2);
    expect(result.workspace.listeners[0].name).toBe("本地草稿");

    const later = reconcileListenerDraft(
      result.workspace,
      latest,
      { ...latest, revision: 3 },
      result.conflictingListenerIds,
    );
    expect(later.conflictingListenerIds).toEqual(["listener-a"]);
    expect(later.workspace.listeners[0].name).toBe("本地草稿");
  });

  it("retains an unsaved certificate lease while another listener refreshes", () => {
    const listenerA = dynamicListener("listener-a", "监听 A", 8080);
    const listenerB = dynamicListener("listener-b", "监听 B", 8081);
    const reference = certificateReference(
      "draft-ca",
      "草稿 CA",
      "upstream_server_trust",
    );
    const draftListener = {
      ...listenerA,
      data_plane: {
        kind: "http" as const,
        settings: {
          ...listenerA.data_plane.settings,
          topology: {
            mode: "remote_server" as const,
            settings: {
              fixed_server: {
                upstream_url: "https://server.test",
                upstream_tls: {
                  verify_hostname: true,
                  server_trust: reference.id,
                  client_identity: null,
                },
              },
            },
          },
        },
      },
    };
    const base = {
      ...workspace,
      listeners: [listenerA, listenerB],
      android_network_profiles: [],
    } as ProxyWorkspace;
    const draft = {
      ...base,
      listeners: [draftListener, listenerB],
      certificate_references: [reference],
    };
    const latest = {
      ...base,
      revision: 2,
      listeners: [listenerA, { ...listenerB, name: "监听 B 外部更新" }],
    };

    const result = reconcileListenerDraft(draft, base, latest);

    expect(result.conflictingListenerIds).toEqual([]);
    expect(result.workspace.certificate_references).toContainEqual(reference);
  });
});
