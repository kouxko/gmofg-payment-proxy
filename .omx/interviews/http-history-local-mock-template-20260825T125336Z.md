# HTTP History to Local Mock Rule - Interview Summary

- Profile: standard
- Context: brownfield
- Rounds: 8
- Final ambiguity: 15%
- Threshold: 20%
- Context snapshot: `.omx/context/http-history-local-mock-template-20260825T124357Z.md`
- Repository branch at crystallization: `codex/intercept-proxy-generalization`
- Repository base HEAD at crystallization: `196449dd1ffca59a3191abcf5ed4c74484cf1a4f`
- Prompt-safe initial context: not_needed

## Confirmed decisions

1. Selecting a real HTTP history event creates a normal HTTP Mock rule draft, not a reusable template-catalog item.
2. The draft is not persisted and cannot affect traffic until the user reviews and saves it.
3. Phase 1 uses the existing exact `PathOrRequestType` condition only.
4. Exact `Method + complete request target` matching is a separately requested follow-up feature.
5. The Mock copies the selected response status, eligible text Body, and every rule-valid response Header.
6. Transport-managed and hop-by-hop Headers are excluded; Rust recomputes `Content-Length`.
7. Phase 1 rejects binary, compressed, or non-losslessly-textual response Bodies instead of creating a corrupt Mock.
8. The action appears per complete HTTP response event and creates one draft for the selected interaction.
9. Multi-interaction HTTP connections remain supported through per-response selection; bulk generation is out of scope.
10. Allowed response data is copied exactly. Phase 1 performs no redaction, filtering, rewriting, or extra sensitive-data warning.
11. HTTP only; Socket Mock generation is out of scope.

## Pressure-pass result

The initial request implied copying a real response Body. Current Exchange history exposes HTTP Body as text, so the interview challenged whether lossy binary conversion was acceptable. The user selected lossless text-only generation and rejected silent corruption. Raw-byte observation and binary Mock generation remain a possible later feature.

## Closure confirmation

The user explicitly confirmed the phase-1 scope, non-goals, decision boundaries, and acceptance contract in Round 8.

## Transcript

- Round 1: chose a normal editable persisted HTTP Mock rule as the output; no separate template library.
- Round 2: chose an unsaved draft that affects traffic only after explicit Save.
- Round 3: chose existing exact request-target matching; Method matching deferred.
- Follow-up: explicitly requested Method + request-target matching as a later feature.
- Round 4: chose status + eligible Body + all rule-valid Headers, with transport Header normalization.
- Round 5: chose lossless text-only Body support; binary/compressed/non-UTF-8 rejected.
- Round 6: chose per-response-event generation, one draft at a time.
- Round 7: chose exact copying without automatic sensitive/dynamic-data handling or extra warning.
- Round 8: confirmed the complete contract.

## Runtime note

OMX deep-interview state persistence could not be activated because an existing `ultragoal` workflow owns the repository workflow state. That existing state was not cleared. The context, transcript, and specification artifacts preserve this interview independently.
