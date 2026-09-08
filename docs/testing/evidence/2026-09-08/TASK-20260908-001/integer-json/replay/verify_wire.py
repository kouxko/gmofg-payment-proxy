#!/usr/bin/env python3
"""Compare archived application bytes with independent Java fixture expectations."""
import hashlib
import json
from pathlib import Path

case = Path(__file__).resolve().parents[1]
wire = case / "outputs/cps-wire"
fixture = case / "resources/test-inputs/examples/protocol-packages/gmofg_cps/tests/fixtures/java-signatures.json"
rows = json.loads(fixture.read_text())
row = next(item for item in rows if item["id"] == "pay-after")
expected = json.loads(row["body"])
expected["data"]["sign"] = row["sign"]
raw = (wire / "app_response.bin").read_bytes()
head, body = raw.split(b"\r\n\r\n", 1)
headers = dict(line.split(b":", 1) for line in head.split(b"\r\n")[1:])
length = next(value for key, value in headers.items() if key.lower() == b"content-length")
assert int(length) == len(body)
actual = json.loads(body)
assert actual == expected
assert type(actual["data"]["result"]["amount"]) is int
assert type(actual["data"]["result"]["amountRmb"]) is int
expected_bytes = json.dumps(expected, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
assert body == expected_bytes, "final body must match exact canonical Java fixture bytes"
assert actual["data"]["sign"] == "F070F340438D40D0BC07295E241EE519"
for name in ("origin_request", "app_response"):
    chunks = json.loads((wire / (name + ".chunks.json")).read_text())
    assert b"".join(bytes(chunk) for chunk in chunks) == (wire / (name + ".bin")).read_bytes()
connection = json.loads((wire / "connection.json").read_text())
assert connection["listener_cleaned_up"] is True
def preserve(path, content):
    if path.exists():
        assert path.read_bytes() == content, f"archived output differs: {path}"
    else:
        with path.open("xb") as output:
            output.write(content)


for path in wire.glob("*.bin"):
    preserve(path.with_suffix(".hex"), (path.read_bytes().hex() + "\n").encode())
preserve(wire / "expected-body.bin", expected_bytes)
preserve(wire / "expected-body.hex", (expected_bytes.hex() + "\n").encode())
report = {
    "result": "PASS", "comparison": "exact final application body bytes vs Java pay-after fixture",
    "amount": actual["data"]["result"]["amount"],
    "amountRmb": actual["data"]["result"]["amountRmb"],
    "signature": actual["data"]["sign"], "body_bytes": len(body),
    "body_sha256": hashlib.sha256(body).hexdigest(), "content_length_correct": True,
    "tcp_chunks_reassemble_exactly": True, "listener_cleaned_up": True,
}
preserve(wire / "comparison.json", (json.dumps(report, indent=2) + "\n").encode())
print(json.dumps(report))
