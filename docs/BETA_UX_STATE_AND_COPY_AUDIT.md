# Relintor Beta activity state and microcopy audit

This document records the user-visible presentation contract. Rust execution, recovery, and verification authorities remain the source of truth; these labels do not alter or infer authority state.

## Activity presentation model

| Authority evidence | User headline | Tone | Primary action | Automatic behavior | Advanced evidence |
|---|---|---|---|---|---|
| Fresh mission, executor ready | Ready to run | Neutral | Run next task | Bounded status polling | Mission/task IDs, ledger, scheduler state |
| Executor unavailable | Antigravity setup required | Warning | Set up Antigravity | Readiness re-check | CLI, authentication, bridge, disk details |
| Dispatch accepted, process not yet proven | Starting Antigravity | Info | None | Fast single-flight polling | Execution phase |
| Owned executor running | Antigravity is working | Info | None; Stop safely is secondary danger | Fast single-flight polling | Turn, steps, tool calls, safe boundary |
| Turn ended at durable boundary | Ready to continue | Neutral | Continue mission | Bounded polling | Continuation events |
| Recovery authority requires review | Recovery review required | Warning | Check recovery safety | Never retries automatically | Disposition, blocker, affected paths, checkpoint |
| Execution finished with evidence | Work finished — verify the evidence | Info | Verify work | Verification state refresh | Evidence and requirement details |
| Verification gap | Verification needs attention | Warning | Resolve/re-run from verification details | No completion claim | Missing, failed, stale, blocked evidence |
| All requirements verified with evidence | Verified Complete | Success | View verification | Bounded polling | Certificate and evidence manifest |
| Backend/persistence unavailable | Mission state unavailable | Danger | Retry authority read | No inferred state | Sanitized diagnostic text |

Impossible combinations are resolved conservatively: RUNNING never exposes Run next task; executor setup never renders Ready; fresh missions never describe a previous attempt; manual recovery never auto-retries; and a `VerifiedComplete` label with missing or contradictory evidence is shown as needs attention.

## Major microcopy changes

| Before | After | Reason |
|---|---|---|
| See what is true now. | Project name, under Mission activity | Tells users what screen and project they are viewing while retaining Relintor's proof language as support. |
| Dispatch next task | Run next task | Uses the user's intent without changing the bounded dispatch operation. |
| Revalidate recovery | Check recovery safety | Explains the purpose instead of naming an internal operation. |
| Revalidate and retry | Removed from the default surface | Avoids combining a safety decision with a potentially unsafe retry. |
| Refresh persisted state | Refresh status, under Advanced controls | Status updates automatically; manual refresh remains a fallback. |
| This mission cannot be called complete | Verification needs attention / Recovery review required | Distinguishes normal waiting from intervention-required blockers. |
| Verification state unavailable | Waiting for evidence | Pre-evidence is a normal sequence state, not a failure. |
| P8 verification authority | Verification | Keeps the authority implementation available in details without making phase names user vocabulary. |
| 0 / 9 tasks; 9 runnable | 0 completed · 9 remaining; Task 1 of 9 | Expresses real discrete progress in familiar terms without inventing a percentage. |
| Mission and task IDs as headings | Project name and task objective | Technical identifiers remain available only in Mission details. |
| Rust ledger: extended path | Ledger path in Mission details, without `\\?\` prefix | Preserves canonical storage while presenting a normal Windows path. |
| Antigravity is ready for execution (large setup card) | Antigravity · Ready (compact status) | Readiness no longer dominates the task once setup succeeds. |
| IDE: not verified while executor READY | Omitted from the ready summary | The CLI/bridge is the required execution capability; optional IDE discovery no longer contradicts readiness. |
| Seal & Build | Seal mission | Accurately describes the immutable handoff; sealing does not itself finish or verify a build. |
| Sealing & building… | Sealing mission… | Gives immediate, truthful feedback for the actual operation. |
| Entitlement UNAVAILABLE | Beta access not confirmed | Explains the user impact without exposing an enum. |
| PENDING_EXTERNAL_ENVIRONMENT | Not available in this closed Beta | Replaces release-gate vocabulary with a truthful product statement. |

## Severity summary

- P0: False completion and unsafe automatic retry were prevented by conservative verification and recovery presentation rules.
- P1: Fresh-run recovery confusion, missing task objectives, executor readiness prominence, and multi-action Activity controls were corrected.
- P2: Technical IDs, extended paths, phase labels, timeline enums, account entitlement language, and standards density were moved behind progressive disclosure or rewritten.
- P3: Focus coverage, semantic tones, responsive summary grids, disabled states, and status announcements were normalized.
