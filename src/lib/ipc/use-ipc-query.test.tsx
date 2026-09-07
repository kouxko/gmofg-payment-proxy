/** 验证查询 Hook 的 loading/error、禁用、刷新和过期 Promise 淘汰。 */

import { act, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useIpcQuery } from "./use-ipc-query";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function QueryProbe({
  queryKey,
  load,
  enabled = true,
  initialData,
}: {
  queryKey: string;
  load: () => Promise<string>;
  enabled?: boolean;
  initialData?: string;
}) {
  const query = useIpcQuery(queryKey, load, initialData, { enabled });
  return (
    <div>
      <output aria-label="data">{query.data ?? "empty"}</output>
      <output aria-label="loading">{String(query.isLoading)}</output>
      <button onClick={() => query.invalidate()}>invalidate</button>
    </div>
  );
}

describe("useIpcQuery request lifetime", () => {
  it("keeps bootstrap data visible while the first refresh is pending", async () => {
    const request = deferred<string>();
    render(
      <QueryProbe
        queryKey="bootstrap"
        initialData="bootstrap snapshot"
        load={() => request.promise}
      />,
    );

    expect(screen.getByLabelText("data")).toHaveTextContent("bootstrap snapshot");
    expect(screen.getByLabelText("loading")).toHaveTextContent("true");

    await act(async () => request.resolve("fresh snapshot"));
    expect(screen.getByLabelText("data")).toHaveTextContent("fresh snapshot");
  });

  it("ignores an older request that finishes after the query key changes", async () => {
    const first = deferred<string>();
    const second = deferred<string>();
    const { rerender } = render(
      <QueryProbe queryKey="first" load={() => first.promise} />,
    );

    await act(async () => first.resolve("first result"));
    expect(screen.getByLabelText("data")).toHaveTextContent("first result");

    rerender(<QueryProbe queryKey="second" load={() => second.promise} />);
    expect(screen.getByLabelText("data")).toHaveTextContent("empty");
    expect(screen.getByLabelText("loading")).toHaveTextContent("true");

    await act(async () => second.resolve("newest"));
    expect(screen.getByLabelText("data")).toHaveTextContent("newest");

    expect(screen.getByLabelText("loading")).toHaveTextContent("false");
  });

  it("does not let a superseded pending request overwrite the latest query", async () => {
    const first = deferred<string>();
    const second = deferred<string>();
    const { rerender } = render(
      <QueryProbe queryKey="first pending" load={() => first.promise} />,
    );

    rerender(
      <QueryProbe queryKey="second pending" load={() => second.promise} />,
    );
    await act(async () => second.resolve("latest result"));
    await act(async () => first.resolve("stale result"));

    expect(screen.getByLabelText("data")).toHaveTextContent("latest result");
  });

  it("invalidates pending data and ignores the response after a detail closes", async () => {
    const request = deferred<string>();
    render(<QueryProbe queryKey="detail" load={() => request.promise} />);

    screen.getByRole("button", { name: "invalidate" }).click();
    await act(async () => request.resolve("late payload"));

    expect(screen.getByLabelText("data")).toHaveTextContent("empty");
    expect(screen.getByLabelText("loading")).toHaveTextContent("false");
  });

  it("does not update state when a pending request resolves after unmount", async () => {
    const request = deferred<string>();
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { unmount } = render(
      <QueryProbe queryKey="unmounted" load={() => request.promise} />,
    );

    unmount();
    await act(async () => request.resolve("too late"));

    expect(consoleError).not.toHaveBeenCalled();
    consoleError.mockRestore();
  });

  it("does not load while disabled", async () => {
    let calls = 0;
    render(
      <QueryProbe
        queryKey="closed"
        enabled={false}
        load={async () => {
          calls += 1;
          return "unexpected";
        }}
      />,
    );
    await act(async () => Promise.resolve());

    expect(calls).toBe(0);
    expect(screen.getByLabelText("data")).toHaveTextContent("empty");
  });
});
