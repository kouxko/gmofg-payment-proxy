from __future__ import annotations

import asyncio
import json
import unittest
from collections.abc import Awaitable, Callable
from typing import Any

from websockets.asyncio.server import serve

from au_eftex.client import MAX_WIRE_MESSAGE_BYTES, ExternalPackageClient


class FakeCodec:
    def frame(self, direction: str, buffer: bytes) -> dict[str, object]:
        return {"status": "need_more"}

    def decode(self, direction: str, frame: bytes) -> dict[str, object]:
        return {}

    def encode(self, direction: str, document: dict[str, object]) -> bytes:
        return b""

    def display(self, direction: str, document: dict[str, object]) -> str:
        return "<p>ok</p>"


class HugeDisplayCodec(FakeCodec):
    def display(self, direction: str, document: dict[str, object]) -> str:
        return "x" * MAX_WIRE_MESSAGE_BYTES


_CLOSE = object()


class FakeSocket:
    def __init__(self) -> None:
        self.incoming: asyncio.Queue[object] = asyncio.Queue()
        self.sent: list[str] = []
        self.closed: list[tuple[int, str]] = []

    async def recv(self) -> str | bytes:
        value = await self.incoming.get()
        if value is _CLOSE:
            raise EOFError("closed")
        assert isinstance(value, (str, bytes))
        return value

    async def send(self, message: str) -> None:
        self.sent.append(message)

    async def close(self, code: int = 1000, reason: str = "") -> None:
        self.closed.append((code, reason))
        self.incoming.put_nowait(_CLOSE)

    def receive(self, value: Any) -> None:
        self.incoming.put_nowait(json.dumps(value, separators=(",", ":")))

    def receive_raw(self, value: str | bytes) -> None:
        self.incoming.put_nowait(value)

    def disconnect(self) -> None:
        self.incoming.put_nowait(_CLOSE)


class FakeConnector:
    def __init__(self) -> None:
        self.sockets: list[FakeSocket] = []
        self.calls: list[tuple[str, int]] = []

    async def __call__(self, url: str, max_size: int) -> FakeSocket:
        self.calls.append((url, max_size))
        socket = FakeSocket()
        self.sockets.append(socket)
        return socket


async def wait_until(predicate: Callable[[], bool]) -> None:
    deadline = asyncio.get_running_loop().time() + 1
    while not predicate():
        if asyncio.get_running_loop().time() >= deadline:
            raise AssertionError("condition timed out")
        await asyncio.sleep(0)


class ExternalPackageClientTests(unittest.IsolatedAsyncioTestCase):
    async def test_real_loopback_websocket_registration_and_dispatch(self) -> None:
        completed: asyncio.Future[tuple[dict[str, object], dict[str, object]]] = (
            asyncio.get_running_loop().create_future()
        )

        async def handler(socket: Any) -> None:
            await socket.send(
                json.dumps(
                    {
                        "jsonrpc": "2.0",
                        "id": "register",
                        "method": "package.register",
                        "params": {"api": 1},
                    },
                    separators=(",", ":"),
                )
            )
            registration = json.loads(await socket.recv())
            await socket.send(
                json.dumps(
                    {
                        "jsonrpc": "2.0",
                        "id": "frame",
                        "method": "hooks.upstream.split_frame",
                        "params": {"buffer_base64": "YWJj"},
                    },
                    separators=(",", ":"),
                )
            )
            frame = json.loads(await socket.recv())
            completed.set_result((registration, frame))
            await socket.wait_closed()

        server = await serve(handler, "127.0.0.1", 0, compression=None)
        port = server.sockets[0].getsockname()[1]
        client = ExternalPackageClient(
            url=f"ws://127.0.0.1:{port}/packages",
            codec=FakeCodec(),
            reconnect_delay=60,
            logger=lambda _: None,
        )
        running = asyncio.create_task(client.run())
        try:
            registration, frame = await asyncio.wait_for(completed, timeout=2)
            self.assertEqual(registration["result"]["package"]["id"], "au-eftex")
            self.assertEqual(frame["result"], {"status": "need_more"})
        finally:
            await client.stop()
            await running
            server.close()
            await server.wait_closed()

    async def test_rpc_logs_safe_method_stage_and_byte_metadata(self) -> None:
        connector = FakeConnector()
        events: list[dict[str, object]] = []
        client = ExternalPackageClient(
            url="ws://127.0.0.1:8765/packages",
            codec=FakeCodec(),
            reconnect_delay=60,
            connector=connector,
            logger=events.append,
        )

        running = asyncio.create_task(client.run())
        await wait_until(lambda: len(connector.sockets) == 1)
        connector.sockets[0].receive(
            {
                "jsonrpc": "2.0",
                "id": "register",
                "method": "package.register",
                "params": {"api": 1},
            }
        )
        await wait_until(lambda: len(connector.sockets[0].sent) == 1)
        connector.sockets[0].receive(
            {
                "jsonrpc": "2.0",
                "id": "frame-1",
                "method": "hooks.upstream.split_frame",
                "params": {"buffer_base64": "YWJj"},
            }
        )
        await wait_until(
            lambda: any(
                event["event"] == "rpc_completed"
                and event.get("method") == "hooks.upstream.split_frame"
                for event in events
            )
        )
        await client.stop()
        await running

        completed = next(
            event
            for event in events
            if event["event"] == "rpc_completed"
            and event.get("method") == "hooks.upstream.split_frame"
        )
        self.assertEqual(completed["method"], "hooks.upstream.split_frame")
        self.assertEqual(completed["direction"], "upstream")
        self.assertEqual(completed["operation"], "split_frame")
        self.assertEqual(completed["input_bytes"], 3)
        self.assertEqual(completed["frame_status"], "need_more")
        self.assertNotIn("YWJj", json.dumps(events))

    async def test_unknown_method_suffix_cannot_inject_sensitive_text_into_logs(self) -> None:
        connector = FakeConnector()
        events: list[dict[str, object]] = []
        client = ExternalPackageClient(
            url="ws://127.0.0.1:8765/packages",
            codec=FakeCodec(),
            reconnect_delay=60,
            connector=connector,
            logger=events.append,
        )

        running = asyncio.create_task(client.run())
        await wait_until(lambda: len(connector.sockets) == 1)
        socket = connector.sockets[0]
        socket.receive(
            {
                "jsonrpc": "2.0",
                "id": "register",
                "method": "package.register",
                "params": {"api": 1},
            }
        )
        await wait_until(lambda: len(socket.sent) == 1)
        socket.receive(
            {
                "jsonrpc": "2.0",
                "id": "unknown",
                "method": "hooks.upstream.4111111111111111",
                "params": {},
            }
        )
        await wait_until(lambda: len(socket.sent) == 2)
        await client.stop()
        await running

        logs = json.dumps(events)
        self.assertNotIn("4111111111111111", logs)
        self.assertTrue(any(event.get("method") == "unknown" for event in events))

    async def test_reconnects_without_sending_unsolicited_registration(self) -> None:
        connector = FakeConnector()
        client = ExternalPackageClient(
            url="ws://127.0.0.1:8765/packages",
            codec=FakeCodec(),
            reconnect_delay=0,
            connector=connector,
        )

        running = asyncio.create_task(client.run())
        await wait_until(lambda: len(connector.sockets) == 1)
        self.assertEqual(connector.sockets[0].sent, [])
        connector.sockets[0].disconnect()
        await wait_until(lambda: len(connector.sockets) >= 2)
        self.assertEqual(connector.calls[0][1], MAX_WIRE_MESSAGE_BYTES)
        await client.stop()
        await running

    async def test_replies_to_proxy_initiated_registration(self) -> None:
        connector = FakeConnector()
        client = ExternalPackageClient(
            url="ws://127.0.0.1:8765/packages",
            codec=FakeCodec(),
            reconnect_delay=0,
            connector=connector,
        )

        running = asyncio.create_task(client.run())
        await wait_until(lambda: len(connector.sockets) == 1)
        socket = connector.sockets[0]
        socket.receive(
            {
                "jsonrpc": "2.0",
                "id": "registration-id",
                "method": "package.register",
                "params": {"api": 1},
            }
        )
        await wait_until(lambda: len(socket.sent) == 1)

        response = json.loads(socket.sent[0])
        self.assertEqual(response["id"], "registration-id")
        self.assertEqual(response["result"]["package"]["id"], "au-eftex")
        await client.stop()
        await running

    async def test_registration_state_resets_after_reconnect(self) -> None:
        connector = FakeConnector()
        client = ExternalPackageClient(
            url="ws://127.0.0.1:8765/packages",
            codec=FakeCodec(),
            reconnect_delay=0,
            connector=connector,
        )

        running = asyncio.create_task(client.run())
        await wait_until(lambda: len(connector.sockets) == 1)
        first = connector.sockets[0]
        first.receive(
            {
                "jsonrpc": "2.0",
                "id": "first",
                "method": "package.register",
                "params": {"api": 1},
            }
        )
        await wait_until(lambda: len(first.sent) == 1)
        first.disconnect()
        await wait_until(lambda: len(connector.sockets) >= 2)
        second = connector.sockets[1]
        second.receive(
            {
                "jsonrpc": "2.0",
                "id": "second",
                "method": "package.register",
                "params": {"api": 1},
            }
        )
        await wait_until(lambda: len(second.sent) == 1)

        self.assertIn("result", json.loads(second.sent[0]))
        await client.stop()
        await running

    async def test_closes_on_oversized_wire_message(self) -> None:
        connector = FakeConnector()
        client = ExternalPackageClient(
            url="ws://127.0.0.1:8765/packages",
            codec=FakeCodec(),
            reconnect_delay=60,
            connector=connector,
        )

        running = asyncio.create_task(client.run())
        await wait_until(lambda: len(connector.sockets) == 1)
        socket = connector.sockets[0]
        socket.receive_raw("x" * (MAX_WIRE_MESSAGE_BYTES + 1))
        await wait_until(lambda: bool(socket.closed))

        self.assertEqual(socket.closed[0][0], 1009)
        await client.stop()
        await running

    async def test_closes_before_sending_an_oversized_wire_response(self) -> None:
        connector = FakeConnector()
        client = ExternalPackageClient(
            url="ws://127.0.0.1:8765/packages",
            codec=HugeDisplayCodec(),
            reconnect_delay=60,
            connector=connector,
        )

        running = asyncio.create_task(client.run())
        await wait_until(lambda: len(connector.sockets) == 1)
        socket = connector.sockets[0]
        socket.receive(
            {
                "jsonrpc": "2.0",
                "id": "register",
                "method": "package.register",
                "params": {"api": 1},
            }
        )
        await wait_until(lambda: len(socket.sent) == 1)
        socket.receive(
            {
                "jsonrpc": "2.0",
                "id": "large-display",
                "method": "document.upstream.render_message",
                "params": {"document": {}},
            }
        )
        await wait_until(lambda: bool(socket.closed))

        self.assertEqual(len(socket.sent), 1)
        self.assertEqual(json.loads(socket.sent[0])["id"], "register")
        self.assertEqual(socket.closed[0][0], 1009)
        await client.stop()
        await running

    async def test_structured_logs_never_include_payloads_credentials_or_exception_text(self) -> None:
        connector = FakeConnector()
        events: list[dict[str, object]] = []
        client = ExternalPackageClient(
            url="ws://127.0.0.1:8765/packages",
            codec=FakeCodec(),
            reconnect_delay=60,
            connector=connector,
            logger=events.append,
        )

        running = asyncio.create_task(client.run())
        await wait_until(lambda: len(connector.sockets) == 1)
        connector.sockets[0].receive_raw('{"secret":"4111111111111111"}')
        await wait_until(lambda: bool(connector.sockets[0].closed))
        await client.stop()
        await running

        logs = json.dumps(events)
        self.assertNotIn("4111111111111111", logs)
        self.assertNotIn("secret", logs)
        self.assertTrue(all("event" in event for event in events))

    async def test_rejects_unsafe_numeric_correlation_id(self) -> None:
        connector = FakeConnector()
        client = ExternalPackageClient(
            url="ws://127.0.0.1:8765/packages",
            codec=FakeCodec(),
            reconnect_delay=60,
            connector=connector,
        )

        running = asyncio.create_task(client.run())
        await wait_until(lambda: len(connector.sockets) == 1)
        socket = connector.sockets[0]
        socket.receive(
            {
                "jsonrpc": "2.0",
                "id": 2**53,
                "method": "package.register",
                "params": {"api": 1},
            }
        )
        await wait_until(lambda: bool(socket.closed))

        self.assertEqual(socket.closed[0][0], 1002)
        self.assertEqual(socket.sent, [])
        await client.stop()
        await running

    def test_rejects_urls_that_can_leak_credentials_or_query_secrets(self) -> None:
        with self.assertRaisesRegex(ValueError, "exact /packages path"):
            ExternalPackageClient(
                url="ws://user:secret@127.0.0.1:8765/packages?token=value",
                codec=FakeCodec(),
            )

        with self.assertRaisesRegex(ValueError, "exact /packages path"):
            ExternalPackageClient(
                url="ws://192.0.2.10:8765/packages",
                codec=FakeCodec(),
            )

        ExternalPackageClient(
            url="ws://192.0.2.10:8765/packages",
            codec=FakeCodec(),
            allow_insecure_remote_ws=True,
        )

        ExternalPackageClient(
            url="wss://packages.example.test/packages",
            codec=FakeCodec(),
        )


if __name__ == "__main__":
    unittest.main()
