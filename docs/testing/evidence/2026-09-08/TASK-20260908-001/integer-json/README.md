# Document integer JSON regression evidence

Task: TASK-20260908-001. This case verifies numeric serialization, configuration
compatibility and HTTP/Wasm callers on the local hotfix branch. Verification is complete: local acceptance PASS and independent review APPROVE.
Individual results are recorded under `outputs/*/run.json`.

## Contract and reproduction

Previously, parsing `0` and encoding the Document produced `0.0`. The RED run
records this mismatch and the nested-array mismatch. The confirmed replacement
contract writes integral values such as `2` and `2.0` as the JSON number `2`,
preserves fractional values and strings, and keeps the existing finite-number
and safe-integer validation. Signed zero retains its existing `-0.0` representation, covered by a regression
test; this hotfix does not introduce a new signed-zero normalization contract.

The domain tests exercise parse/Deserialize/construction and all three JSON
serialization entry points. Current active test inputs are embedded in the
archived test source. HTTP tests modify a different field, then compare the
complete body and unchanged integer fields. No-op, nonmatching and same-value
rules preserve the original body. Wasm tests load actual json-pretty/CPS
components; the existing ISO8583 component checks the Socket bridge. The CPS
loopback test additionally verifies the final application response and sign.

## Environment and commands

See `inputs/environment.json` for tool versions, branch and base commit. Run
commands from the repository root with the recorded Rust toolchain, Deno/Node,
and `wasm32-wasip2` installed. Existing Cargo lockfiles define dependencies.
Actual Wasm fixtures, source snapshots and provenance are under `resources/`.

Every recorded command uses `replay/run.py <unique-label> -- <command> ...`.
The runner saves exact stdout/stderr, times, exit status, source hashes before
and after execution, and relevant environment overrides. Choose a new label
for a rerun; existing output directories are not overwritten. Each
`outputs/<label>/run.json` contains the full repeatable command. A RED result is
the pre-fix reproducer, not a successful acceptance result.

For CPS wire capture, set `CPS_WIRE_EVIDENCE_DIR` to a fresh directory in this
case's `outputs/`. This test uses ephemeral loopback listeners, records the
actual addresses and TCP chunks, and checks listener cleanup. No production
endpoint, device, credentials, or persistent application configuration is used.

## Parent resources and unchanged business expectations

Derived from TASK-20260907-004, case `cps-checksum`, at
[the original evidence](../../../2026-09-07/TASK-20260907-004/cps-checksum/README.md).
The original Java response models and Gson are reused from its immutable
`resources/cps-source/` and `resources/gson-2.9.0.jar`. The Java oracle verifies
the source/JAR checksums before execution. The current active
`examples/protocol-packages/gmofg_cps/tests/fixtures/java-signatures.json`
retains the original canonical/signature expectations. This task changes the
Host's numeric representation and adds strict wire assertions; it does not
change the CPS signing algorithm or rewrite historical evidence.

## Evidence applicability

- Pure domain, configuration and static checks: input/source snapshots,
  commands, output and exact assertions; TCP chunks/screenshots are N/A.
- Wasm Host: actual components, JSON input/output and source tests; network
  chunks are N/A because these tests invoke the component in process.
- CPS loopback: raw requests/responses, chunks, rules, signature, addresses and
  cleanup; external TLS/mTLS and real payment processing are NOT_RUN.
- Frontend: existing jsdom interaction tests, lint and typecheck. Native desktop
  UI/real-device acceptance and screenshots are NOT_RUN.
- Remote CI, pushing, MR creation, deployment and release are NOT_RUN.

## Final results

Domain 109; Application 477; Package Runtime 23; HTTP pipeline 22;
frontend 27; Java oracle 14: PASS. Final configuration rerun: 8 PASS.
Actual Wasm bridges, unchanged-body paths, Socket frame and CPS wire passed.
CPS body: 337 bytes, SHA-256
`ca04cc545c8fa408c8660e5e85672ec6dcbe64fa6ad8394894a4bb4b4e8f4e90`.
Typecheck, lint, architecture, source-size, fmt and strict affected-crate
Clippy passed. Release binding generator built and ran twice: fresh and
deterministic, no generated TS changes. Independent review: [APPROVE](outputs/review.md).
The accepted run list is in `metadata.json`. Earlier RED, initial whitespace
assertion and formatting/Clippy failures remain as diagnostic history only.

`resources/source-provenance.json` and `resources/input-provenance.json`
map 50 source snapshots and 17 file inputs to their current source and SHA-256.
`resources/hotfix.patch` applies on the recorded base commit and was checked in
reverse against the tested tree. Restore the recorded source/base and input
paths in an isolated checkout for historical reproduction, then use commands
and environment overrides from each run record. Generated component sources
remain in that base checkout; actual tested binaries are archived here.
`python3 docs/testing/evidence/2026-09-08/TASK-20260908-001/integer-json/replay/verify_wire.py`
compares saved wire without a running app. It verifies existing derived files
without overwriting them. `checksums.json` records the completed archive.

