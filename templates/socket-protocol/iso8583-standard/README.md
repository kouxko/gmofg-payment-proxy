# ISO 8583:1987 ASCII starter

This directory is the source of the bundled `iso8583-ascii-standard@1.0.0` ZIP.

- `manifest.json` owns the exact identity and upstream/downstream JSON schemas.
- `protocol.js` implements the two-byte big-endian frame header and fixed directional exports.
- `display.js` renders the decoded MTI as untrusted HTML.

The current starter decodes the four-character message type and preserves all other original frame
bytes. Changing `message_type` rewrites those four ASCII bytes and retains the original length and
payload. Extend the schema and codec together when adding data elements; do not add a second
manifest or alternate execution entry.

Files under `samples/` are authoring vectors and are not included in the compiled package ZIP.
