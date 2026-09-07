# Independent integrated architecture review

Reviewer: `/root/final_architecture_review` (independent architect, did not implement this batch). This file is the leader's faithful written record of the review returned through the agent channel; the reviewer was read-only and did not write a file.

Verdict: **APPROVE for implemented/local-verified scope**. No P0, P1 or in-scope P2 findings. Architectural status: WATCH only for real-device acceptance and archive metadata closure.

- Protected saved recovery converges on ControlLeaseCoordinator.recover and returns WaitingForDesktopControl before beginStart/enqueue/lease mutation. Unprotected device-only recovery preserves the original path and clears lease.
- Job and saved Service recovery consume the same admission. Idle generation CAS cleanup cannot close a newer start.
- apply_active validates watchdog epoch/deadline and registry desiredRunning/running/current generation within the coordinator/registry ownership boundary before enqueue. Lock ordering and stale callbacks preserve the replacement instance.
- Both v2 operation whitelists explicitly support apply_active. Old Companion rejects unknown operation; there is no fallback to ordinary apply.
- LAN endpoint reconciliation requires Active owner and matching serial/epoch active_runtime. Protected live updates carry the existing control lease with apply_active; explicit start/apply remains the manual recovery path with a new epoch.
- Observed Stopped/Faulted changes an Active owner to CleanupRequired while retaining the owner/epoch for explicit CAS.
- Positive/negative regressions cover waiting, disabled protection, new manual epoch, matching live hot apply, expired/cleared/old epoch/old generation rejection and no automatic retry.
- Evidence checked: application 432/432, infrastructure 474/474, focused 5/5, JVM 45/45, lint/release/signature/alignment/four ABI PASS. Instrumentation compiled; real Android LocalSocket/TUN execution remains NOT_RUN due no device.

Reviewed source anchors: ControlLeaseCoordinator.kt:24-71; ControlLeaseWatchdog.kt:25-45; VpnRuntimeRegistry.kt:61-75; InterceptVpnServiceControl.kt:45-78; InterceptVpnService.kt:167-185; ResumeVpnJobService.kt:13-31; Companion ControlProtocol.kt:37-40 and :67-75; application android.rs:281-300; android_adb endpoint_reconciliation.rs:33-107 and owner.rs:308-348.

Review follow-ups: leader must update root README/metadata from RUNNING under task lock; retain real-device NOT_RUN; optional android-companion README contradiction “planned/already implemented” can be changed to “formal desktop control uses”. Leader applied that wording-only correction after review, with a separate snapshot/static check. No source-code change followed this review.

Earlier accepted Task001 #1/#2/#4-#9/#12 and Task002 contracts remain under their prior independent APPROVE evidence and this integration's preservation checks. Display 1 MiB, fixed five-second protection, strict routing, bootstrap/header and Listener Draft contracts were not replaced. This approval does not upgrade any historical real-device NOT_RUN to PASS.
