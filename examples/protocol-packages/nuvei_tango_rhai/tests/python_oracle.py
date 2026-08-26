#!/usr/bin/env python3
from __future__ import annotations

import base64
import json
import sys
from pathlib import Path


PACKAGE_ROOT = Path(__file__).resolve().parents[1]
REPOSITORY_ROOT = PACKAGE_ROOT.parents[2]
PYTHON_PACKAGE = REPOSITORY_ROOT / "examples" / "external-packages" / "nuvei_tango_json"
sys.path.insert(0, str(PYTHON_PACKAGE))

from nuvei_tango_json.codec import TangoJsonCodec  # noqa: E402


def main() -> None:
    direction = sys.argv[1]
    frame = bytes.fromhex(sys.argv[2])
    codec = TangoJsonCodec(context_key=b"r" * 32)
    result: dict[str, object] = {"frame": codec.frame(direction, frame)}
    try:
        document = codec.decode(direction, frame)
    except (TypeError, ValueError) as error:
        result["decode"] = {"status": "error", "message": str(error)}
    else:
        result["decode"] = {
            "status": "ok",
            "frame_length": int(document["frame_length"]["value"]),
            "control_header_hex": base64.b64decode(
                document["control_header"]["value_base64"], validate=True
            ).hex(),
            "sequence": document["sequence"]["value"],
            "message_type": document["message_type"]["value"],
            "json_preview_type": document["json_preview"]["type"],
            "encoding_context_type": document["encoding_context"]["type"],
            "encode_hex": codec.encode(direction, document).hex(),
        }
    print(json.dumps(result, sort_keys=True, separators=(",", ":")))


if __name__ == "__main__":
    main()
