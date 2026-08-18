import { describe, expect, expectTypeOf, it } from "vitest";
import {
  commands,
  type AndroidRuntimeOwnerMode,
  type AndroidRuntimeOwnerSource,
  type AndroidRuntimeOwnerState,
  type AndroidRuntimeOwnerTransitionReason,
  type AndroidRuntimeOwnerViewModel,
} from "@/generated/rust-types";

type CommandResult = Awaited<ReturnType<typeof commands.deviceNetworkRuntimeOwner>>;
type CommandData = Extract<CommandResult, { status: "ok" }>["data"];

const generatedOwnerFixture = {
  serial: "device-a",
  epoch: "11111111-1111-4111-8111-111111111111",
  mode: "adb_reverse",
  profile_id: "profile-a",
  state: "active",
  source: "start",
  transition_reason: "activation_confirmed",
  updated_at: "2026-08-17T00:00:00Z",
} satisfies AndroidRuntimeOwnerViewModel;

describe("generated Android runtime owner contract", () => {
  it("keeps the command payload aligned with the exported DTO", () => {
    expectTypeOf<CommandData>().toEqualTypeOf<AndroidRuntimeOwnerViewModel | null>();
    expect(generatedOwnerFixture).toMatchObject({
      serial: "device-a",
      mode: "adb_reverse",
      state: "active",
      transition_reason: "activation_confirmed",
    });
  });

  it("keeps owner mode, state, source and reason as closed generated unions", () => {
    expectTypeOf<AndroidRuntimeOwnerMode>().toEqualTypeOf<
      "device_only" | "lan" | "adb_reverse"
    >();
    expectTypeOf<AndroidRuntimeOwnerState>().toEqualTypeOf<
      "active" | "uncertain" | "waiting_reconnect" | "cleanup_required" | "stop_failed" | "faulted"
    >();
    expectTypeOf<AndroidRuntimeOwnerSource>().toEqualTypeOf<
      "start" | "apply" | "recovery"
    >();
    expectTypeOf<AndroidRuntimeOwnerTransitionReason>().toEqualTypeOf<
      | "activation_confirmed"
      | "activation_uncertain"
      | "reverse_preparation"
      | "reverse_cleanup_required"
      | "device_disconnected"
      | "device_reconnected"
      | "stop_failed"
      | "recovered_from_storage"
      | "lan_endpoint_reapplied"
      | "lan_endpoint_faulted"
    >();
  });
});
