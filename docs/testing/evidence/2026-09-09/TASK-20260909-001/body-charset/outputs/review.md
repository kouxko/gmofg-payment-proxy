# Code Review Summary

**Task:** TASK-20260909-001  
**Branch:** `hotfix/fix-http-body-charset-preview`  
**Base:** `gitlab/develop` / `1c88aeb8`  
**Files Reviewed:** 16 task source artifacts, plus task/architecture/evidence documents  
**Total Issues:** 0

## Spec and root-cause compliance

PASS.

- The implementation fixes the confirmed root cause: non-UTF-8 HTTP bodies previously reached Exchange observation only as `from_utf8_lossy` text. `ObservedContext::Http` now borrows the existing authoritative `wire_body`, and received/encoded/sent/failed observation events carry its hex only when `body_is_utf8=false`.
- The producer checks the combined header/body/document/display/error plus wire-hex budget before allocating the hex projection. UTF-8 bodies keep the previous projection and are not duplicated as hex.
- The desktop consumer decodes only the observation preview, reusing the existing Auto UTF-8/Shift-JIS codec selection from the HTTP Content-Type. Business `HttpContext`, protocol Decode/Rules/Encode, framing, and forwarded bytes are unchanged.
- Missing/unsupported charset, Raw, and compressed bodies retain the prior lossy projection under the explicitly documented boundary. Supported codecs with invalid bytes expose a preview error instead of silently accepting replacement characters.
- `body_is_utf8` remains false after successful Shift-JIS preview decoding, so Application still rejects using that preview as a UTF-8 Mock replacement body.
- No new retry, success default, business fallback, protocol-package path, Listener codec override, persistence, or release behavior was introduced.

## Severity

- P0: 0
- P1: 0
- P2: 0

## Quality, security, and resource review

PASS.

- No hardcoded secret, injection/XSS path, swallowed supported-codec failure, broad catch, or business error masking was introduced.
- The tracing representation remains primitive and bounded. Exact wire bytes do not enter the public DTO; the final DTO contains only the decoded/lossy preview and preserves the non-UTF-8 eligibility flag.
- The new infrastructure helper is read-only and uses the existing parser/codec contract. Its compatibility fallback is narrowly limited to missing/unsupported/Raw/compressed observation previews, keeps the old evidence visible, and has primary/fallback/error tests.
- Architecture boundary checks pass; no architecture concern needs escalation from this code/spec/security lane.

## Validation evidence

- The 16 archived task-source SHA-256 values match the current worktree.
- Formal affected verification on those hashes: Exchange 33, codec 6, Mock 1, consumer 21, frontend 8 = 69 tests PASS.
- Strict Clippy for all targets in the four affected Rust crates, rustfmt, TypeScript `tsc --noEmit`, ESLint, architecture boundary/docs checks, and `git diff --check` all exit 0 with `source_stable=true`.
- Reviewer spot rerun: Exchange crate 33 tests PASS with `source_stable=true` on the current worktree.
- RED evidence captures the original producer/consumer failure; final DTO and comparison evidence confirm readable Japanese text across four context stages, preserved `body_is_utf8=false`, and matching input bytes.

## Declared limits (not findings)

- Existing in-memory observations are not retroactively rebuilt; a new request is required.
- Listener-forced codec behavior, additional charset support, protocol packages, real server/device replay, CI, installation, and release remain outside this hotfix.

## Recommendation

**APPROVE**

The task is merge-ready from the code/spec/security review lane. No P0/P1/P2 finding remains.
