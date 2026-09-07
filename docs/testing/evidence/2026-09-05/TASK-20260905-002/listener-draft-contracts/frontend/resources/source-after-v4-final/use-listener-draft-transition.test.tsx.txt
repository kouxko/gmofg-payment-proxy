// @vitest-environment jsdom

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ListenerDraft } from "@/generated/rust-types";
import { dynamicListener } from "./listeners-view.test-support";

const mocks = vi.hoisted(() => ({ listenerDraftTransition: vi.fn() }));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));
vi.mock("@/lib/ipc/client", () => ({ callCommand: (value: unknown) => value }));

import { useListenerDraftTransition } from "./use-listener-draft-transition";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("useListenerDraftTransition", () => {
  let listener: ListenerDraft;
  let scopeVersion: number;
  let operationLocked: boolean;
  let applied: ListenerDraft[];
  let errors: unknown[];

  beforeEach(() => {
    vi.clearAllMocks();
    listener = dynamicListener() as ListenerDraft;
    scopeVersion = 1;
    operationLocked = false;
    applied = [];
    errors = [];
  });

  function setup() {
    return renderHook(() => useListenerDraftTransition({
      getContext: () => operationLocked ? undefined : {
        workspaceId: "workspace-1",
        workspaceRevision: 1,
        listener,
        scopeVersion,
      },
      apply: (draft) => {
        listener = draft;
        applied.push(draft);
      },
      onError: (reason) => errors.push(reason),
    }));
  }

  it("serially composes independent intents and publishes only the batch endpoint", async () => {
    const app = deferred<ListenerDraft>();
    const server = deferred<ListenerDraft>();
    const withApp = { ...listener, name: "app tls" };
    const withBoth = { ...listener, name: "app and server tls" };
    mocks.listenerDraftTransition
      .mockReturnValueOnce(app.promise)
      .mockReturnValueOnce(server.promise);
    const { result } = setup();

    act(() => {
      result.current.enqueue({ type: "app_tls", enabled: true }, null);
      result.current.enqueue({ type: "server_tls", enabled: true }, null);
    });
    expect(mocks.listenerDraftTransition).toHaveBeenCalledTimes(1);

    await act(async () => app.resolve(withApp));
    await waitFor(() => expect(mocks.listenerDraftTransition).toHaveBeenCalledTimes(2));
    expect(mocks.listenerDraftTransition.mock.calls[1][0]).toEqual(withApp);
    expect(applied).toEqual([]);
    await act(async () => server.resolve(withBoth));

    await waitFor(() => expect(applied).toEqual([withBoth]));
    expect(result.current.transitioning).toBe(false);
  });

  it("applies the final value for rapid changes of the same control", async () => {
    const first = deferred<ListenerDraft>();
    const second = deferred<ListenerDraft>();
    mocks.listenerDraftTransition
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const { result } = setup();

    act(() => {
      result.current.enqueue({ type: "app_tls", enabled: true }, null);
      result.current.enqueue({ type: "app_tls", enabled: false }, null);
    });
    await act(async () => first.resolve({ ...listener, name: "enabled" }));
    await waitFor(() => expect(mocks.listenerDraftTransition).toHaveBeenCalledTimes(2));
    const disabled = { ...listener, name: "disabled" };
    await act(async () => second.resolve(disabled));

    await waitFor(() => expect(applied).toEqual([disabled]));
  });

  it("cancels the old batch synchronously and lets a new scope finish independently", async () => {
    const oldRequest = deferred<ListenerDraft>();
    const newRequest = deferred<ListenerDraft>();
    mocks.listenerDraftTransition
      .mockReturnValueOnce(oldRequest.promise)
      .mockReturnValueOnce(newRequest.promise);
    const { result } = setup();

    act(() => {
      result.current.enqueue({ type: "processing", mode: "scripted" }, null);
      result.current.enqueue({ type: "socket_topology", mode: "local_responder" }, null);
      scopeVersion += 1;
      result.current.invalidate();
      result.current.enqueue({ type: "processing", mode: "direct" }, null);
    });
    const latest = { ...listener, name: "latest scope" };
    await act(async () => newRequest.resolve(latest));
    await waitFor(() => expect(applied).toEqual([latest]));

    await act(async () => oldRequest.resolve({ ...listener, name: "stale" }));
    expect(applied).toEqual([latest]);
    expect(mocks.listenerDraftTransition).toHaveBeenCalledTimes(2);
  });

  it("stops a failed batch without sending its queued tail", async () => {
    const request = deferred<ListenerDraft>();
    mocks.listenerDraftTransition.mockReturnValueOnce(request.promise);
    const { result } = setup();
    const failure = new Error("transition failed");

    act(() => {
      result.current.enqueue({ type: "app_tls", enabled: true }, null);
      result.current.enqueue({ type: "server_tls", enabled: true }, null);
    });
    await act(async () => request.reject(failure));

    await waitFor(() => expect(errors).toEqual([failure]));
    expect(mocks.listenerDraftTransition).toHaveBeenCalledTimes(1);
    expect(applied).toEqual([]);
    expect(result.current.transitioning).toBe(false);
  });

  it("blocks intents from the stale plane while allowing the plane selector to change again", () => {
    mocks.listenerDraftTransition.mockReturnValue(new Promise(() => undefined));
    const { result } = setup();

    act(() => {
      result.current.enqueue({ type: "data_plane", kind: "http" }, null);
      result.current.enqueue({ type: "server_tls", enabled: true }, null);
      result.current.enqueue({ type: "data_plane", kind: "socket" }, null);
    });

    expect(result.current.dataPlanePending).toBe(true);
    expect(result.current.serverControlsBlocked).toBe(true);
    expect(mocks.listenerDraftTransition).toHaveBeenCalledTimes(1);
  });

  it("blocks stale Server TLS after LocalResponder is queued", () => {
    mocks.listenerDraftTransition.mockReturnValue(new Promise(() => undefined));
    const { result } = setup();

    act(() => {
      result.current.enqueue({ type: "socket_topology", mode: "local_responder" }, null);
      result.current.enqueue({ type: "server_tls", enabled: true }, null);
    });

    expect(result.current.dataPlanePending).toBe(false);
    expect(result.current.serverControlsBlocked).toBe(true);
    expect(mocks.listenerDraftTransition).toHaveBeenCalledTimes(1);
  });

  it("does not publish a transition response after unmount", async () => {
    const request = deferred<ListenerDraft>();
    mocks.listenerDraftTransition.mockReturnValueOnce(request.promise);
    const { result, unmount } = setup();

    act(() => result.current.enqueue({ type: "app_tls", enabled: true }, null));
    unmount();
    await act(async () => request.resolve({ ...listener, name: "late" }));

    expect(applied).toEqual([]);
    expect(errors).toEqual([]);
  });
});
