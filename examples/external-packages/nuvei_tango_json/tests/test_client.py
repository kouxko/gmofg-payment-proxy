from __future__ import annotations

import asyncio
import base64
import json
import unittest
from typing import Any

from websockets.asyncio.server import serve

from nuvei_tango_json.client import ExternalPackageClient
from nuvei_tango_json.codec import TangoJsonCodec

from test_codec import synthetic_frame


class ExternalPackageClientTests(unittest.IsolatedAsyncioTestCase):
    async def test_loopback_websocket_registers_with_proxy_contract(self) -> None:
        completed: asyncio.Future[dict[str, object]] = asyncio.get_running_loop().create_future()

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
            completed.set_result(json.loads(await socket.recv()))
            await socket.wait_closed()

        server = await serve(handler, "127.0.0.1", 0, compression=None)
        port = server.sockets[0].getsockname()[1]
        client = ExternalPackageClient(
            url=f"ws://127.0.0.1:{port}/packages",
            codec=TangoJsonCodec(context_key=b"w" * 32),
            reconnect_delay=60,
            logger=lambda _: None,
        )
        running = asyncio.create_task(client.run())
        try:
            response = await asyncio.wait_for(completed, timeout=2)
            self.assertEqual(response["result"]["package"]["id"], "nuvei-tango-json")
        finally:
            await client.stop()
            await running
            server.close()
            await server.wait_closed()

    def test_rejects_remote_plaintext_websocket_by_default(self) -> None:
        with self.assertRaisesRegex(ValueError, "loopback ws or wss"):
            ExternalPackageClient(
                url="ws://10.0.0.8:8765/packages",
                codec=TangoJsonCodec(context_key=b"w" * 32),
            )

    async def test_rpc_logs_safe_direction_stage_sizes_and_result(self) -> None:
        completed: asyncio.Future[None] = asyncio.get_running_loop().create_future()
        frame = synthetic_frame()
        frame_base64 = base64.b64encode(frame).decode("ascii")
        invalid_frame_base64 = base64.b64encode(b"synthetic-invalid-frame").decode("ascii")

        async def handler(socket: Any) -> None:
            for request in (
                {
                    "jsonrpc": "2.0",
                    "id": "register",
                    "method": "package.register",
                    "params": {"api": 1},
                },
                {
                    "jsonrpc": "2.0",
                    "id": "split",
                    "method": "hooks.upstream.split_frame",
                    "params": {"buffer_base64": frame_base64},
                },
                {
                    "jsonrpc": "2.0",
                    "id": "decode",
                    "method": "hooks.upstream.decrypt_message",
                    "params": {"frame_base64": frame_base64},
                },
                {
                    "jsonrpc": "2.0",
                    "id": "decode-error",
                    "method": "hooks.downstream.decrypt_message",
                    "params": {"frame_base64": invalid_frame_base64},
                },
            ):
                await socket.send(json.dumps(request, separators=(",", ":")))
                await socket.recv()
            completed.set_result(None)
            await socket.wait_closed()

        server = await serve(handler, "127.0.0.1", 0, compression=None)
        port = server.sockets[0].getsockname()[1]
        events: list[dict[str, object]] = []
        client = ExternalPackageClient(
            url=f"ws://127.0.0.1:{port}/packages",
            codec=TangoJsonCodec(context_key=b"l" * 32),
            reconnect_delay=60,
            logger=events.append,
        )
        running = asyncio.create_task(client.run())
        try:
            await asyncio.wait_for(completed, timeout=2)
        finally:
            await client.stop()
            await running
            server.close()
            await server.wait_closed()

        split_started = next(
            event
            for event in events
            if event.get("event") == "rpc_started"
            and event.get("method") == "hooks.upstream.split_frame"
        )
        self.assertEqual(split_started["direction"], "upstream")
        self.assertEqual(split_started["operation"], "frame")
        self.assertEqual(split_started["input_bytes"], len(frame))

        split_completed = next(
            event
            for event in events
            if event.get("event") == "rpc_completed"
            and event.get("method") == "hooks.upstream.split_frame"
        )
        self.assertEqual(split_completed["outcome"], "ok")
        self.assertEqual(split_completed["frame_status"], "complete")
        self.assertEqual(split_completed["consumed_bytes"], len(frame))
        self.assertIsInstance(split_completed["duration_ms"], int)

        failed = next(
            event
            for event in events
            if event.get("event") == "rpc_completed"
            and event.get("method") == "hooks.downstream.decrypt_message"
        )
        self.assertEqual(failed["direction"], "downstream")
        self.assertEqual(failed["input_bytes"], len(b"synthetic-invalid-frame"))
        self.assertEqual(failed["outcome"], "error")
        self.assertEqual(failed["jsonrpc_error_code"], -32002)
        self.assertEqual(failed["error_code"], "DECODE_FAILED")

        serialized = json.dumps(events, ensure_ascii=False)
        self.assertNotIn(frame_base64, serialized)
        self.assertNotIn(invalid_frame_base64, serialized)
        self.assertNotIn("synthetic-pan", serialized)
        self.assertNotIn("json_preview", serialized)


if __name__ == "__main__":
    unittest.main()
