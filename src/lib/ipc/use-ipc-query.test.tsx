import { act, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
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
}: {
  queryKey: string;
  load: () => Promise<string>;
  enabled?: boolean;
}) {
  const query = useIpcQuery(queryKey, load, undefined, { enabled });
  return (
    <div>
      <output aria-label="data">{query.data ?? "empty"}</output>
      <output aria-label="loading">{String(query.isLoading)}</output>
      <button onClick={() => query.invalidate()}>invalidate</button>
    </div>
  );
}

describe("useIpcQuery request lifetime", () => {
  it("ignores an older request that finishes after the query key changes", async () => {
    const first = deferred<string>();
    const second = deferred<string>();
    const { rerender } = render(
      <QueryProbe queryKey="first" load={() => first.promise} />,
    );

    rerender(<QueryProbe queryKey="second" load={() => second.promise} />);
    await act(async () => second.resolve("newest"));
    expect(screen.getByLabelText("data")).toHaveTextContent("newest");

    await act(async () => first.resolve("stale"));
    expect(screen.getByLabelText("data")).toHaveTextContent("newest");
  });

  it("invalidates pending data and ignores the response after a detail closes", async () => {
    const request = deferred<string>();
    render(<QueryProbe queryKey="detail" load={() => request.promise} />);

    screen.getByRole("button", { name: "invalidate" }).click();
    await act(async () => request.resolve("late payload"));

    expect(screen.getByLabelText("data")).toHaveTextContent("empty");
    expect(screen.getByLabelText("loading")).toHaveTextContent("false");
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
