import { describe, expect, it } from "vitest";
import type { SocketRelayDraftSettings } from "@/generated/rust-types";
import { socketWorkingMode } from "./socket-listener-model";
import { localResponderListener, socketListener } from "./listeners-view.test-support";

function socketSettings(): SocketRelayDraftSettings {
  const listener = socketListener();
  if (listener.data_plane.kind !== "socket") throw new Error("expected Socket");
  return listener.data_plane.settings;
}

describe("socketWorkingMode", () => {
  it("projects direct Relay, scripted Relay, and LocalResponder modes", () => {
    const direct = socketSettings();
    expect(socketWorkingMode(direct)).toBe("raw_relay");
    expect(socketWorkingMode({
      ...direct,
      processing: { mode: "scripted", settings: { package: null } },
    })).toBe("protocol_relay");
    const local = localResponderListener();
    if (local.data_plane.kind !== "socket") throw new Error("expected Socket");
    expect(socketWorkingMode(local.data_plane.settings)).toBe("local_response");
  });
});
