import { ExternalPackageClient } from "./client.ts";

const url = Deno.env.get("EXTERNAL_PACKAGE_URL") ?? "ws://127.0.0.1:8765/packages";
const reconnectDelayMs = parsePositiveInteger(
  Deno.env.get("RECONNECT_DELAY_MS") ?? "1000",
  "RECONNECT_DELAY_MS",
);

const client = new ExternalPackageClient({ url, reconnectDelayMs });
const abort = (): void => client.stop();
Deno.addSignalListener("SIGINT", abort);
if (Deno.build.os !== "windows") Deno.addSignalListener("SIGTERM", abort);

try {
  await client.run();
} finally {
  Deno.removeSignalListener("SIGINT", abort);
  if (Deno.build.os !== "windows") Deno.removeSignalListener("SIGTERM", abort);
}

function parsePositiveInteger(value: string, name: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 0) {
    throw new Error(`${name} must be a non-negative integer`);
  }
  return parsed;
}
