from __future__ import annotations

import asyncio
import os

from .client import ExternalPackageClient
from .codec import TangoJsonCodec


def _flag(name: str) -> bool:
    value = os.environ.get(name, "0")
    if value not in {"0", "1"}:
        raise RuntimeError(f"{name} must be 0 or 1")
    return value == "1"


async def _run() -> None:
    client = ExternalPackageClient(
        url=os.environ.get("EXTERNAL_PACKAGE_URL", "ws://127.0.0.1:8765/packages"),
        codec=TangoJsonCodec(),
        reconnect_delay=float(os.environ.get("RECONNECT_DELAY_SECONDS", "1")),
        allow_insecure_remote_ws=_flag("NUVEI_TANGO_ALLOW_INSECURE_REMOTE_WS"),
    )
    try:
        await client.run()
    finally:
        await client.stop()


def main() -> None:
    try:
        asyncio.run(_run())
    except KeyboardInterrupt:
        return


if __name__ == "__main__":
    main()
