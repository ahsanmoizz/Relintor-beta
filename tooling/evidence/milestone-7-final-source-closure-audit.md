# Relintor P7 — Final Source Closure Audit

## Verdict

`MILESTONE_7_FINAL_SOURCE_REPAIR_REQUIRED`

The prior independent P7 repair is materially correct. The final narrow source
inspection found four remaining authority issues in the repaired delta.

Do not reopen the whole phase. Repair only P7-FC-01 through P7-FC-04.
P8 must remain not started until this closure gate passes.

## P7-FC-01 — A caller can still fabricate a successful action and finish a task

`record_action` is public. Any Rust caller can:

1. `start_task`;
2. optionally call `authorize_action`;
3. call `record_action` with a caller-created successful `ActionResult`;
4. call `finish_task`.

A successful `record_action` sets `execution_source`, and `finish_task` treats
that as sufficient proof that execution occurred.

This bypasses the intended rule that implementation completion must arise from
the Rust-owned adapter orchestration path.

Repair:
- an arbitrary caller-created `ActionResult` must not be sufficient to unlock
  `FinishedAwaitingVerification`;
- completion authorization must be set only by the adapter-owned execution
  result path (or another explicitly Rust-owned trusted executor path);
- keep low-level telemetry/action recording separate from completion authority.

## P7-FC-02 — Adapter orchestration dispatches before scheduler authorization and uses stale time

`execute_next_with_adapter` currently starts a task and calls
`dispatch_with_adapter` before calling `authorize_action`.

It then calls `finish_task(..., now_ms)` using the original start timestamp,
even though the adapter result contains its real `ended_at_ms`.

Consequences:
- the execution boundary does not perform the scheduler authorization immediately
  before mutable dispatch;
- a process that runs past lease expiry can still be evaluated with the stale
  start time.

Repair:
- authorize the Rust-owned Antigravity execution action before dispatch;
- after dispatch, use the adapter-owned end timestamp for lease/expiry and
  completion decisions;
- a process that finishes after lease expiry/time budget must not become
  `FinishedAwaitingVerification`.

## P7-FC-03 — External-modification expected-path matching still uses lexical `starts_with`

`detect_external_modification` still classifies changed paths using string
prefix matching.

Example:
- expected: `D:\Project`
- changed: `D:\Project-Evil\file`

The sibling can be treated as expected.

Repair:
- use the same safe component-aware/canonical containment semantics used by
  lease path authorization;
- prefix siblings and traversal must be unexpected external modifications.

## P7-FC-04 — Public P7 constructor still bypasses verified registry admission

`from_p6_handoff_with_registry` correctly binds the mission to the verified
registry.

But `pub fn from_p6_handoff(...)` remains publicly callable and only checks that
the supplied trusted signer set is non-empty.

That leaves a second public admission path that bypasses the exact-registry gate.

Repair:
- make the unbound constructor private / `pub(crate)` / test-only, OR remove it;
- production and external callers must use the verified-registry constructor;
- update acceptance fixtures to build a signed registry and use the same
  verified admission path rather than preserving a weaker public API for tests.

## Closure rule

When the two new executable tests, the static closure audit, the previous
6-test independent P7 source audit, the 24-test P7 acceptance corpus, and the
workspace/desktop/security regressions all pass, P7 can be certified for
continuation with the already-carried real-Antigravity and cross-platform debt.
