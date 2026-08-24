"""AU EFTEX external package process entry point."""

from __future__ import annotations

import asyncio
import os
import stat
from collections.abc import Mapping
from pathlib import Path

from .client import ExternalPackageClient
from .codec import EftexCodec


def _load_secret(prefix: str, expected_bytes: int, environ: Mapping[str, str]) -> bytes:
    file_name = f"{prefix}_FILE"
    hex_name = f"{prefix}_HEX"
    file_value = environ.get(file_name)
    hex_value = environ.get(hex_name)
    if (file_value is None) == (hex_value is None):
        raise RuntimeError(f"configure exactly one of {file_name} or {hex_name}")
    if file_value is not None:
        path = Path(file_value)
        if os.name != "nt" and stat.S_IMODE(path.stat().st_mode) & (
            stat.S_IRWXG | stat.S_IRWXO
        ):
            raise RuntimeError(f"{file_name} must reference an owner-only file")
        try:
            value = path.read_text(encoding="ascii").strip()
        except (OSError, UnicodeError) as error:
            raise RuntimeError(f"unable to read {file_name}") from error
    else:
        value = hex_value
    assert value is not None
    try:
        decoded = bytes.fromhex(value)
    except ValueError as error:
        raise RuntimeError(f"{prefix} must be hexadecimal") from error
    if len(decoded) != expected_bytes:
        raise RuntimeError(f"{prefix} must contain exactly {expected_bytes} bytes")
    return decoded


def _reconnect_delay() -> float:
    value = os.environ.get("RECONNECT_DELAY_SECONDS", "1")
    try:
        delay = float(value)
    except ValueError as error:
        raise RuntimeError("RECONNECT_DELAY_SECONDS must be a number") from error
    if delay < 0:
        raise RuntimeError("RECONNECT_DELAY_SECONDS must be non-negative")
    return delay


def _allow_insecure_remote_ws(environ: Mapping[str, str]) -> bool:
    value = environ.get("AU_EFTEX_ALLOW_INSECURE_REMOTE_WS", "0")
    if value not in {"0", "1"}:
        raise RuntimeError("AU_EFTEX_ALLOW_INSECURE_REMOTE_WS must be 0 or 1")
    return value == "1"


async def _run() -> None:
    bdk = _load_secret("AU_EFTEX_BDK", 16, os.environ)
    os.environ.pop("AU_EFTEX_BDK_HEX", None)
    codec = EftexCodec(
        bdk=bdk,
    )
    client = ExternalPackageClient(
        url=os.environ.get(
            "EXTERNAL_PACKAGE_URL",
            "ws://127.0.0.1:8765/packages",
        ),
        codec=codec,
        reconnect_delay=_reconnect_delay(),
        allow_insecure_remote_ws=_allow_insecure_remote_ws(os.environ),
    )
    try:
        await client.run()
    finally:
        await client.stop()


def main() -> None:
    try:
        asyncio.run(_run())
    except KeyboardInterrupt:
        # Ctrl-C is an expected operator shutdown. `asyncio.run` has already
        # cancelled and drained `_run`, whose `finally` closes the WebSocket;
        # do not turn that clean lifecycle into a traceback and exit code 1.
        return


if __name__ == "__main__":
    main()
