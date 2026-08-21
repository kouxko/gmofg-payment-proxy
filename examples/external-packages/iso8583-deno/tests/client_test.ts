import { ExternalPackageClient, type SocketFactory, type WebSocketPeer } from "../src/client.ts";
import { assert, assertEquals } from "./assert.ts";

class FakeSocket extends EventTarget implements WebSocketPeer {
  readonly sent: string[] = [];
  readonly url: string;
  readyState = WebSocket.CONNECTING;

  constructor(url: string) {
    super();
    this.url = url;
  }

  send(data: string): void {
    this.sent.push(data);
  }

  close(): void {
    this.readyState = WebSocket.CLOSED;
    this.dispatchEvent(new CloseEvent("close"));
  }

  open(): void {
    this.readyState = WebSocket.OPEN;
    this.dispatchEvent(new Event("open"));
  }

  receive(value: unknown): void {
    this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify(value) }));
  }

  receiveRaw(value: unknown): void {
    this.dispatchEvent(new MessageEvent("message", { data: value }));
  }
}

Deno.test("client reconnects and leaves Ping/Pong plus registration initiation to Proxy", async () => {
  const sockets: FakeSocket[] = [];
  const factory: SocketFactory = (url) => {
    const socket = new FakeSocket(url);
    sockets.push(socket);
    queueMicrotask(() => socket.open());
    return socket;
  };
  const client = new ExternalPackageClient({
    url: "ws://127.0.0.1:8765/packages",
    reconnectDelayMs: 0,
    socketFactory: factory,
  });

  const running = client.run();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assertEquals(sockets.length, 1);
  assertEquals(sockets[0]?.sent, []);
  sockets[0]?.close();
  await waitUntil(() => sockets.length >= 2);
  assert(sockets.length >= 2, "client should reconnect");
  client.stop();
  await running;
});

Deno.test("client replies to JSON-RPC requests and ignores non-text frames safely", async () => {
  let socket: FakeSocket | undefined;
  const factory: SocketFactory = (url) => {
    socket = new FakeSocket(url);
    queueMicrotask(() => socket?.open());
    return socket;
  };
  const client = new ExternalPackageClient({
    url: "ws://127.0.0.1:8765/packages",
    reconnectDelayMs: 0,
    socketFactory: factory,
  });
  const running = client.run();
  await new Promise((resolve) => setTimeout(resolve, 0));
  socket?.receiveRaw(new Uint8Array([1, 2, 3]));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assertEquals(socket?.sent, []);
  socket?.receive({
    jsonrpc: "2.0",
    id: "register-1",
    method: "package.register",
    params: { api: 1 },
  });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assertEquals(JSON.parse(socket?.sent[0] ?? "null").id, "register-1");
  client.stop();
  await running;
});

Deno.test("client rejects URL credentials and query data so logs cannot expose secrets", () => {
  let rejected = false;
  try {
    new ExternalPackageClient({ url: "ws://user:secret@127.0.0.1:8765/packages?token=value" });
  } catch {
    rejected = true;
  }
  assert(rejected, "credential-bearing URLs must be rejected");
});

async function waitUntil(predicate: () => boolean): Promise<void> {
  const deadline = Date.now() + 1_000;
  while (!predicate()) {
    if (Date.now() >= deadline) throw new Error("condition timeout");
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
}
