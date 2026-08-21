import { createRpcDispatcher } from "./rpc.ts";
import type { JsonRpcRequest } from "./types.ts";

const MAX_WIRE_MESSAGE_BYTES = 1024 * 1024;
const encoder = new TextEncoder();

export interface WebSocketPeer extends EventTarget {
  readonly readyState: number;
  send(data: string): void;
  close(code?: number, reason?: string): void;
}

export type SocketFactory = (url: string) => WebSocketPeer;

export interface ExternalPackageClientOptions {
  readonly url: string;
  readonly reconnectDelayMs?: number;
  readonly socketFactory?: SocketFactory;
}

/**
 * Long-running external package peer.
 *
 * Deno's native WebSocket implementation handles Ping/Pong control frames. The
 * application processes text JSON-RPC only and never emits an unsolicited
 * registration message: package.register is always initiated by Proxy.
 */
export class ExternalPackageClient {
  readonly #url: string;
  readonly #reconnectDelayMs: number;
  readonly #socketFactory: SocketFactory;
  #activeSocket?: WebSocketPeer;
  #stopped = false;

  constructor(options: ExternalPackageClientOptions) {
    const url = new URL(options.url);
    if (
      (url.protocol !== "ws:" && url.protocol !== "wss:") ||
      url.pathname !== "/packages" ||
      url.username !== "" ||
      url.password !== "" ||
      url.search !== "" ||
      url.hash !== ""
    ) {
      throw new Error("external package URL must use ws/wss and the exact /packages path");
    }
    this.#url = url.toString();
    this.#reconnectDelayMs = options.reconnectDelayMs ?? 1_000;
    this.#socketFactory = options.socketFactory ?? ((address) => new WebSocket(address));
  }

  async run(): Promise<void> {
    this.#stopped = false;
    let attempt = 0;
    while (!this.#stopped) {
      attempt++;
      const socket = this.#socketFactory(this.#url);
      this.#activeSocket = socket;
      log("connection_attempt", { attempt, url: this.#url });
      try {
        await this.#serveConnection(socket);
      } catch (error) {
        log("connection_error", { attempt, error: safeError(error) });
      } finally {
        if (this.#activeSocket === socket) this.#activeSocket = undefined;
      }
      if (!this.#stopped) await delay(this.#reconnectDelayMs);
    }
  }

  stop(): void {
    this.#stopped = true;
    this.#activeSocket?.close(1000, "external package stopped");
  }

  async #serveConnection(socket: WebSocketPeer): Promise<void> {
    await waitForOpen(socket);
    log("connected", { url: this.#url });
    const dispatch = createRpcDispatcher();
    const pending = new Set<Promise<void>>();
    const onMessage = (event: Event): void => {
      const task = this.#handleMessage(socket, dispatch, event as MessageEvent).finally(() => {
        pending.delete(task);
      });
      pending.add(task);
    };
    socket.addEventListener("message", onMessage);
    try {
      await waitForClose(socket);
    } finally {
      socket.removeEventListener("message", onMessage);
      await Promise.allSettled(pending);
      log("disconnected", { url: this.#url });
    }
  }

  async #handleMessage(
    socket: WebSocketPeer,
    dispatch: ReturnType<typeof createRpcDispatcher>,
    event: MessageEvent,
  ): Promise<void> {
    if (typeof event.data !== "string") {
      log("ignored_non_text_message", { kind: typeof event.data });
      return;
    }
    if (encoder.encode(event.data).length > MAX_WIRE_MESSAGE_BYTES) {
      socket.close(1009, "JSON-RPC message exceeds 1 MiB");
      return;
    }
    let request: JsonRpcRequest;
    try {
      request = parseRequest(event.data);
    } catch (error) {
      log("protocol_error", { error: safeError(error) });
      socket.close(1002, "invalid JSON-RPC request");
      return;
    }
    const startedAt = performance.now();
    const response = await dispatch(request);
    const encoded = JSON.stringify(response);
    if (encoder.encode(encoded).length > MAX_WIRE_MESSAGE_BYTES) {
      log("response_too_large", { id: request.id, method: request.method });
      socket.close(1009, "JSON-RPC response exceeds 1 MiB");
      return;
    }
    if (socket.readyState === WebSocket.OPEN) socket.send(encoded);
    log("rpc_completed", {
      id: request.id,
      method: request.method,
      outcome: "error" in response ? "error" : "ok",
      duration_ms: Math.round(performance.now() - startedAt),
    });
  }
}

function parseRequest(text: string): JsonRpcRequest {
  const value: unknown = JSON.parse(text);
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("JSON-RPC request must be an object");
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record).sort();
  if (JSON.stringify(keys) !== JSON.stringify(["id", "jsonrpc", "method", "params"])) {
    throw new Error("JSON-RPC request contains missing or unknown fields");
  }
  if (record.jsonrpc !== "2.0" || typeof record.method !== "string") {
    throw new Error("unsupported JSON-RPC request");
  }
  if (typeof record.id !== "string" && typeof record.id !== "number") {
    throw new Error("JSON-RPC id must be a string or number");
  }
  if (typeof record.id === "number" && !Number.isSafeInteger(record.id)) {
    throw new Error("numeric JSON-RPC id must be a safe integer");
  }
  return record as unknown as JsonRpcRequest;
}

function waitForOpen(socket: WebSocketPeer): Promise<void> {
  if (socket.readyState === WebSocket.OPEN) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const opened = (): void => {
      cleanup();
      resolve();
    };
    const failed = (): void => {
      cleanup();
      reject(new Error("WebSocket closed before opening"));
    };
    const cleanup = (): void => {
      socket.removeEventListener("open", opened);
      socket.removeEventListener("close", failed);
      socket.removeEventListener("error", failed);
    };
    socket.addEventListener("open", opened, { once: true });
    socket.addEventListener("close", failed, { once: true });
    socket.addEventListener("error", failed, { once: true });
  });
}

function waitForClose(socket: WebSocketPeer): Promise<void> {
  if (socket.readyState === WebSocket.CLOSED) return Promise.resolve();
  return new Promise((resolve) =>
    socket.addEventListener("close", () => resolve(), { once: true })
  );
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function safeError(error: unknown): string {
  return error instanceof Error ? error.message : "unknown error";
}

/** Structured operational logs contain correlation metadata, never message bodies or keys. */
function log(event: string, details: Record<string, unknown>): void {
  console.log(
    JSON.stringify({ timestamp: new Date().toISOString(), level: "info", event, ...details }),
  );
}
