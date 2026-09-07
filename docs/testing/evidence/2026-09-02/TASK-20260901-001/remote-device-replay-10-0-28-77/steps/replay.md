# Replay steps

1. Build all repository Components with `pnpm build:protocol-packages`.
2. Import the five `.wasm` files from `resources/` into the Windows App and enable their exact versions.
3. Through MCP `environment_candidate_create` and `environment_candidate_apply`, create Workspace
   `Remote Wasm Replay Display Fix 20260902` with the mappings recorded in `outputs/remote-state.json`.
4. Select the Workspace and start ports 8084 through 8087 in the App. AU EFTEX was run separately on 8083.
5. On Mac `10.0.34.59`, bind controlled TCP upstreams on ports 18084, 18085, 18087 and 18088. Each upstream
   reads exactly the request recorded in `inputs/vectors.json`, compares it byte-for-byte, then returns the exact
   response. AU EFTEX uses port 18086.
6. Connect to `10.0.28.77:<listener port>`, send the matching request without a client half-close, and read the
   complete expected response. Compare the client response byte-for-byte.
7. Query MCP `exchange_observation_query` for every Listener. Require `opened`, upstream
   `received/encoded/sent`, downstream `received/encoded/sent`, and `closed/outcome=completed`; reject any failed
   event.
8. For ISO Deno require two Display strings containing `<td>1000</td>`. For Nuvei Rhai require two Display
   strings containing `<table class="protocol-document-nested">` and no `<pre>`.
9. Query `diagnostics_query` after cursor 132 and `entry_status_list`. Require no warning/error or
   `socket_failure`, exact byte counters, running state and `fault_reason=null`.

## Final rules replay

1. Import the five final Wasm files, including both Nuvei 1.0.1 packages.
2. Start `replay/rule_harness.py` on Mac `10.0.34.59`; keep ports 18084 through 18088 listening.
3. Through the MCP candidate preview/apply flow, create the five listeners and five enabled
   `proxy_to_upstream` rules in `inputs/rule-candidate.json`, then select the committed Workspace.
4. Start all five listeners in the App and verify `state=running` plus `fault_reason=null`.
5. Run `python3 replay/rule_client.py`. Require five hit responses, four byte-preserving miss responses and four
   invalid requests returning no bytes.
6. Read every rule lifecycle. Require each `hit_count` to change from 0 to 1 and remain 1 after miss/invalid cases.
7. Query all 13 Exchanges and diagnostics. Require 9 completed Exchanges; require the other 4 to match the exact
   expected fail-closed stage/code and to send 0 bytes upstream. Do not require zero diagnostic errors because the
   four intentionally invalid Frames must remain observable errors.
8. For both Nuvei 1.0.1 packages, require hit and miss, upstream and downstream Display HTML to contain
   `protocol-document-nested` and not contain `<pre>`.
9. Leave the five remote listeners and the controlled upstream harness running for user inspection.
