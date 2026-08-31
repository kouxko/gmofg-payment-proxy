# Authoring a Socket Sidecar package

1. Copy the three root files from `iso8583-standard`.
2. Change the exact identity and recursive schemas in `manifest.json`.
3. Implement both directions of Frame, Decode and Encode in `protocol.js`.
4. Implement both Display exports in `display.js`.
5. ZIP the file contents at the archive root, without a containing directory.

Keep framing independent per direction. `complete.consumedBytes` must be positive and no greater
than the current buffer length. Decode returns natural JSON values. Encode must preserve unchanged
bytes when the Document is unchanged and return the complete frame when it changes.

JavaScript modules may import only relative package modules. Package code has no filesystem,
network, process, environment-variable or native-module access. The package itself initiates the
WebSocket registration; the Proxy never opens a second package protocol.

There is no compatibility mode. An archive that does not match package API 1 must be corrected and
re-imported.
