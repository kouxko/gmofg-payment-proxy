#!/usr/bin/env python3
"""Replay an external AU EFTEX transaction trace through the current package.

The trace remains outside the repository. This verifier reads it in memory,
checks both DUKPT directions, decrypts and parses each ISO8583 message, then
re-encodes the exact original wire bytes. Its output contains only lengths,
MTIs and boolean verification results.
"""

from __future__ import annotations

import argparse
import base64
import json
import re
import sys
from dataclasses import asdict, dataclass
from pathlib import Path


PACKAGE_ROOT = (
    Path(__file__).resolve().parents[1] / "examples" / "external-packages" / "au_eftex"
)
if str(PACKAGE_ROOT) not in sys.path:
    sys.path.insert(0, str(PACKAGE_ROOT))

from Crypto.Cipher import DES3  # noqa: E402 - package path is established above

from au_eftex.codec import EftexCodec, pad_message  # noqa: E402
from au_eftex.crypto import (  # noqa: E402
    derive_data_request_key,
    derive_data_response_key,
    derive_ipek,
    derive_transaction_key,
)
from au_eftex.rpc import RpcDispatcher, create_rpc_dispatcher  # noqa: E402


HEADER_BYTES = 39
_HEX_LINE = re.compile(r"^[0-9A-Fa-f]+$")


class TraceFormatError(ValueError):
    """The supplied trace is incomplete or contradicts its declared metadata."""


@dataclass(frozen=True)
class TraceVerificationResult:
    request_mti: str
    response_mti: str
    request_frame_bytes: int
    response_frame_bytes: int
    request_wire_round_trip: bool
    response_wire_round_trip: bool
    request_document_round_trip: bool
    response_document_round_trip: bool
    request_rpc_round_trip: bool
    response_rpc_round_trip: bool
    fragmented_frame_contract: bool
    dukpt_derivation_matches: bool


@dataclass(frozen=True)
class _TraceVectors:
    bdk: bytes
    ksn: bytes
    ipek: bytes
    transaction_key: bytes
    request_data_key: bytes
    response_data_key: bytes
    request_padded_clear: bytes
    request_frame: bytes
    response_padded_clear: bytes
    response_frame: bytes


def verify_trace_file(path: Path) -> TraceVerificationResult:
    """Verify one trace file without copying its contents into the repository."""

    try:
        text = path.read_text(encoding="ascii")
    except UnicodeDecodeError as error:
        raise TraceFormatError("AU EFTEX trace must be ASCII text") from error
    return verify_trace_text(text)


def verify_trace_text(text: str) -> TraceVerificationResult:
    vectors = _parse_vectors(text)
    ipek = derive_ipek(vectors.bdk, vectors.ksn)
    transaction_key = derive_transaction_key(ipek, vectors.ksn)
    request_data_key = DES3.adjust_key_parity(derive_data_request_key(transaction_key))
    response_data_key = DES3.adjust_key_parity(derive_data_response_key(transaction_key))
    _require_equal("Initial Device Key", ipek, vectors.ipek)
    _require_equal("DUKPT transaction key", transaction_key, vectors.transaction_key)
    _require_equal("request Data key", request_data_key, vectors.request_data_key)
    _require_equal("response Data key", response_data_key, vectors.response_data_key)

    codec = EftexCodec(bdk=vectors.bdk, context_key=bytes(32))
    request = codec.decrypt_frame("upstream", vectors.request_frame)
    response = codec.decrypt_frame("downstream", vectors.response_frame)
    if request.header[28:38] != vectors.ksn or response.header[28:38] != vectors.ksn:
        raise TraceFormatError("H01 KSN does not match the trace KSN")
    _require_equal("request padded plaintext", pad_message(request.message), vectors.request_padded_clear)
    _require_equal(
        "response padded plaintext",
        pad_message(response.message),
        vectors.response_padded_clear,
    )

    request_wire = codec.encrypt_frame(
        "upstream",
        request.header,
        request.message,
        length_prefix_mode=request.length_prefix_mode,
    )
    response_wire = codec.encrypt_frame(
        "downstream",
        response.header,
        response.message,
        length_prefix_mode=response.length_prefix_mode,
    )
    _require_equal("request wire round-trip", request_wire, vectors.request_frame)
    _require_equal("response wire round-trip", response_wire, vectors.response_frame)

    request_document_wire = codec.encode(
        "upstream", codec.decode("upstream", vectors.request_frame)
    )
    response_document_wire = codec.encode(
        "downstream", codec.decode("downstream", vectors.response_frame)
    )
    _require_equal("request Document round-trip", request_document_wire, vectors.request_frame)
    _require_equal("response Document round-trip", response_document_wire, vectors.response_frame)
    request_rpc_round_trip, response_rpc_round_trip = _verify_rpc_contract(
        codec,
        vectors.request_frame,
        vectors.response_frame,
    )
    fragmented_frame_contract = _verify_fragmented_frame_contract(
        codec,
        vectors.request_frame,
    )

    return TraceVerificationResult(
        request_mti=_mti(request.message, "request"),
        response_mti=_mti(response.message, "response"),
        request_frame_bytes=len(vectors.request_frame),
        response_frame_bytes=len(vectors.response_frame),
        request_wire_round_trip=True,
        response_wire_round_trip=True,
        request_document_round_trip=True,
        response_document_round_trip=True,
        request_rpc_round_trip=request_rpc_round_trip,
        response_rpc_round_trip=response_rpc_round_trip,
        fragmented_frame_contract=fragmented_frame_contract,
        dukpt_derivation_matches=True,
    )


def _verify_rpc_contract(
    codec: EftexCodec,
    request_frame: bytes,
    response_frame: bytes,
) -> tuple[bool, bool]:
    dispatch = create_rpc_dispatcher(codec)
    _rpc_result(
        dispatch(
            {
                "jsonrpc": "2.0",
                "id": "register",
                "method": "package.register",
                "params": {"api": 1},
            }
        ),
        "package.register",
    )
    results = []
    for direction, frame in (
        ("upstream", request_frame),
        ("downstream", response_frame),
    ):
        encoded = base64.b64encode(frame).decode("ascii")
        boundary = _rpc_result(
            dispatch(
                {
                    "jsonrpc": "2.0",
                    "id": f"{direction}-frame",
                    "method": f"hooks.{direction}.split_frame",
                    "params": {"buffer_base64": encoded},
                }
            ),
            f"hooks.{direction}.split_frame",
        )
        if boundary != {"status": "complete", "consumed_bytes": len(frame)}:
            raise TraceFormatError(f"{direction} frame hook returned an unexpected boundary")
        document = _rpc_result(
            dispatch(
                {
                    "jsonrpc": "2.0",
                    "id": f"{direction}-decode",
                    "method": f"hooks.{direction}.decrypt_message",
                    "params": {"frame_base64": encoded},
                }
            ),
            f"hooks.{direction}.decrypt_message",
        )["document"]
        rendered = _rpc_result(
            dispatch(
                {
                    "jsonrpc": "2.0",
                    "id": f"{direction}-display",
                    "method": f"document.{direction}.render_message",
                    "params": {"document": document},
                }
            ),
            f"document.{direction}.render_message",
        )
        if not isinstance(rendered.get("html"), str) or not rendered["html"]:
            raise TraceFormatError(f"{direction} display hook returned empty HTML")
        rebuilt = _rpc_result(
            dispatch(
                {
                    "jsonrpc": "2.0",
                    "id": f"{direction}-encode",
                    "method": f"hooks.{direction}.encrypt_message",
                    "params": {"document": document},
                }
            ),
            f"hooks.{direction}.encrypt_message",
        )
        try:
            rebuilt_frame = base64.b64decode(rebuilt["frame_base64"], validate=True)
        except (KeyError, TypeError, ValueError) as error:
            raise TraceFormatError(f"{direction} encode hook returned invalid Base64") from error
        _require_equal(f"{direction} JSON-RPC wire round-trip", rebuilt_frame, frame)
        results.append(True)
    return results[0], results[1]


def _rpc_result(response: dict[str, object], method: str) -> dict[str, object]:
    error = response.get("error")
    if error is not None:
        raise TraceFormatError(f"{method} failed through the external package contract")
    result = response.get("result")
    if not isinstance(result, dict):
        raise TraceFormatError(f"{method} returned an invalid result")
    return result


def _verify_fragmented_frame_contract(codec: EftexCodec, request_frame: bytes) -> bool:
    dispatch = create_rpc_dispatcher(codec)
    _rpc_result(
        dispatch(
            {
                "jsonrpc": "2.0",
                "id": "fragment-register",
                "method": "package.register",
                "params": {"api": 1},
            }
        ),
        "package.register",
    )
    prefixed_body = len(request_frame).to_bytes(2, "big") + request_frame
    prefixed_total = (len(request_frame) + 2).to_bytes(2, "big") + request_frame
    for case, frame in (
        ("raw", request_frame),
        ("u16-body", prefixed_body),
        ("u16-total", prefixed_total),
    ):
        first_read = frame[: min(313, len(frame) - 1)]
        partial = _rpc_frame(dispatch, f"fragment-{case}", first_read)
        if partial != {"status": "need_more"}:
            raise TraceFormatError(f"{case} 313-byte fragment did not return need_more")
        complete = _rpc_frame(dispatch, f"complete-{case}", frame)
        if complete != {"status": "complete", "consumed_bytes": len(frame)}:
            raise TraceFormatError(f"{case} complete frame boundary was not preserved")
    return True


def _rpc_frame(
    dispatch: RpcDispatcher,
    request_id: str,
    frame: bytes,
) -> dict[str, object]:
    encoded = base64.b64encode(frame).decode("ascii")
    return _rpc_result(
        dispatch(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "hooks.upstream.split_frame",
                "params": {"buffer_base64": encoded},
            }
        ),
        "hooks.upstream.split_frame",
    )


def _parse_vectors(text: str) -> _TraceVectors:
    bdk = _hex_value(text, "BDK", 16)
    ksn = _hex_value(text, "KSN", 10)
    request_frame = _hex_block(text, "Encrypted Message (Including Header)")
    if len(request_frame) <= HEADER_BYTES:
        raise TraceFormatError("request frame does not contain H01 and ciphertext")
    response_ciphertext = _hex_block(text, "Encrypted Data")
    return _TraceVectors(
        bdk=bdk,
        ksn=ksn,
        ipek=_hex_value(text, "Initial Device Key", 16),
        transaction_key=_hex_value(text, "DUKPT Key", 16),
        request_data_key=_hex_value(text, "Data Key Request", 16),
        response_data_key=_hex_value(text, "Data Key Response", 16),
        request_padded_clear=_hex_block(text, "Data to Cipher"),
        request_frame=request_frame,
        response_padded_clear=_hex_block(text, "Decrypted Message"),
        response_frame=request_frame[:HEADER_BYTES] + response_ciphertext,
    )


def _hex_value(text: str, label: str, expected_bytes: int) -> bytes:
    match = re.search(
        rf"(?m)^{re.escape(label)}\s*:\s*([0-9A-Fa-f]+)",
        text,
    )
    if match is None:
        raise TraceFormatError(f"trace is missing {label}")
    value = bytes.fromhex(match.group(1))
    if len(value) != expected_bytes:
        raise TraceFormatError(f"{label} must contain {expected_bytes} bytes")
    return value


def _hex_block(text: str, label: str) -> bytes:
    lines = text.splitlines()
    heading = re.compile(rf"^{re.escape(label)}\s+Length\((\d+)\)\s*$")
    for index, line in enumerate(lines):
        match = heading.match(line.strip())
        if match is None:
            continue
        declared = int(match.group(1))
        chunks: list[str] = []
        for candidate in lines[index + 1 :]:
            stripped = candidate.strip()
            if not stripped and not chunks:
                continue
            if not _HEX_LINE.fullmatch(stripped):
                break
            chunks.append(stripped)
        if not chunks:
            raise TraceFormatError(f"{label} does not contain a hexadecimal block")
        value = bytes.fromhex("".join(chunks))
        if len(value) != declared:
            raise TraceFormatError(
                f"{label} declared {declared} bytes but contains {len(value)}"
            )
        return value
    raise TraceFormatError(f"trace is missing {label} Length(...)")


def _require_equal(label: str, actual: bytes, expected: bytes) -> None:
    if actual != expected:
        raise TraceFormatError(f"{label} does not match the current AU EFTEX package")


def _mti(message: bytes, direction: str) -> str:
    try:
        mti = message[:4].decode("ascii")
    except UnicodeDecodeError as error:
        raise TraceFormatError(f"{direction} MTI is not ASCII") from error
    if len(mti) != 4 or not mti.isdigit():
        raise TraceFormatError(f"{direction} MTI must contain four digits")
    return mti


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path, help="Path to the external transaction trace")
    return parser.parse_args()


def main() -> None:
    arguments = _arguments()
    try:
        result = verify_trace_file(arguments.trace)
    except (OSError, TraceFormatError) as error:
        raise SystemExit(f"AU EFTEX TRACE FAILED: {error}") from None
    print(json.dumps(asdict(result), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
