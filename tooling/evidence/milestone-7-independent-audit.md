# Relintor Milestone 7 — Independent Source Audit

## Verdict

`MILESTONE_7_REPAIR_REQUIRED`

The existing 24-test P7 acceptance corpus is useful and the scheduler core is
substantially implemented, but the production-path source review found several
authority gaps not covered by that corpus.

This is a focused P7 repair. Do not reopen P6 and do not start P8.

## P7-IA-01 — Desktop "Dispatch next task" does not dispatch any agent work

The Tauri `execution_step` command currently:
1. loads the execution run;
2. validates authority identity;
3. calls `start_task`;
4. persists the ledger.

It never calls the Antigravity adapter, never records an adapter result, never
finishes or early-stops the task, and therefore cannot advance implementation.

The UI label "Dispatch next task" is stronger than the behavior.

Repair:
- introduce a Rust-owned `execute_next_with_adapter<A: AntigravityAdapter>` style
  orchestration function;
- issue/start the task, validate the lease/packet, dispatch through the adapter,
  classify the actual result, record it, and transition the task truthfully;
- use MockAdapter only in deterministic tests;
- production must use a real P3 adapter when available, otherwise return an
  explicit external-environment blocker without leaving a fake Running task;
- do not claim real Antigravity smoke PASS unless it actually runs.

## P7-IA-02 — The durable execution ledger accepts coherent tampering

`ExecutionRun::restore_json` only deserializes and checks
`ledger_version`.

A modified ledger can change task states, budgets, scopes, usage counters,
watchdog state, attempts, continuations, or leases and still be accepted. The
desktop then checks only the high-level P6 authority identity.

Repair:
- create a canonical execution-ledger integrity envelope;
- authenticate the persisted ledger with a Rust-owned local integrity key
  (prefer an existing OS-keychain/local authority primitive);
- a plain recomputable self-hash is not sufficient for an adversarial tamper
  claim;
- on restore validate ledger identity, P6 authority binding, task set,
  task/attempt/lease invariants, packet/lease digests, budgets, event sequence,
  and integrity authentication before returning trusted state;
- corrupted/tampered ledgers fail closed. P9 will later own recovery, not P7.

## P7-IA-03 — Continuation/retry can reset task budgets

`ContinuationRecord.budget_remaining` is calculated, but `start_next_turn`
does not apply it to the task and the next `start_task` rebuilds a packet from
the task's original full `usage_budget`.

This permits a new turn/attempt to regain tool, step, time, and cost allowance.

Repair:
- maintain task-level remaining budget separately from per-attempt telemetry;
- continuation and retries consume the same bounded task budget;
- a new turn must not reset budget;
- attempts may have sub-budgets, but their sum cannot exceed the sealed P7 task
  budget.

## P7-IA-04 — Cost budgets are modeled but not enforced

`UsageBudget.cost_micros` exists, but `authorize_action` checks tool calls,
steps, wall time, and attempt count only.

Repair:
- if a cost budget is configured, enforce accumulated measured/estimated cost
  before the next mutable action;
- do not invent cost when unavailable;
- if policy requires a hard cost ceiling and cost is unavailable, fail according
  to an explicit policy rather than silently treating it as free.

## P7-IA-05 — Task completion can bypass scheduler authority

`finish_task` can mark a task `FinishedAwaitingVerification` regardless of
whether:
- the task was runnable;
- dependencies were complete;
- a running attempt exists;
- an active matching lease exists;
- any execution action occurred.

That can bypass the dependency scheduler and create a false P7 implementation
state before P8.

Repair:
- completion transition requires the task to be Running under the current valid
  attempt/packet/lease;
- dependencies must still be satisfied;
- consumed/revoked/stale leases cannot finish a task;
- completion must be caused by the Rust-owned adapter result path, not an
  arbitrary caller.

P8 will still independently verify evidence; this gate only prevents false P7
execution state.

## P7-IA-06 — Path authorization is vulnerable to lexical prefix/traversal bypass

`LeaseScope::allows_path` uses lowercase string `starts_with`.

Examples such as:
- `<workspace>/../outside/file`
- a sibling whose name shares the workspace prefix

can pass lexical checks without being inside the authorized path.

Mutable actions with an empty `paths` list are also accepted.

Repair:
- canonicalize/resolve the workspace and candidate path safely;
- reject `..` escape, prefix-sibling escape, symlink/reparse escape where the
  platform can resolve it;
- compare path components, not string prefixes;
- a mutable filesystem action must declare the affected path(s), unless the
  specific Rust-owned tool policy has a separate bounded resource scope.

## P7-IA-07 — Adapter dispatch fabricates a successful process result

`dispatch_with_adapter` currently calls `reconcile_exit` with a synthetic
`ProcessResult` containing:
- executable `"mock-adapter"`;
- exit code `0`.

This is production code, not a test-only helper.

Repair:
- never synthesize successful process exit in production orchestration;
- consume the real adapter/session/process-exit result;
- preserve actual exit code, forced status, timestamps, stdout/stderr bounds,
  and failure classification;
- mock success belongs only in MockAdapter/test fixtures.

## P7-IA-08 — P7 trust input is not actually bound to the sealed registry

`from_p6_handoff` receives `TrustedSignerSet` but only checks that the signer
set is non-empty.

The desktop loads the verified production registry and then discards the
registry itself, passing only the trusted key set.

Repair:
- bind P7 initialization to the exact verified standards registry identity used
  by the sealed mission;
- verify registry ID/version/digest against the trusted production registry
  before creating an execution run;
- a non-empty unrelated signer set must not satisfy the gate;
- do not reopen P6 semantics; this is a P7 admission check.

## P7-IA-09 — Safe-parallel acceptance does not represent production task scope

The acceptance test manually edits execution-task scopes so two tasks become
non-conflicting.

Production `from_p6_handoff` assigns every task the whole workspace plus the
shared `"workspace"` resource, so production planning is always serialized.

Serialization is safe, but claiming G-02 fully verified from the synthetic
scope mutation overstates the product path.

Repair options:
- keep production conservative/serialized and record G-02 as capability-tested
  but not exercised by real sealed task scope, OR
- add a trusted P6/P7 scope derivation contract that can prove disjoint scopes.

Do not infer narrow mutable file scopes from LLM text alone.

## Required independent repair gate

The included `independent_p7_source_audit.rs` covers the machine-testable
authority gaps:
- task completion bypass;
- traversal/empty-path authorization;
- continuation budget reset;
- cost budget enforcement;
- tampered ledger restore.

The included desktop static audit checks that the production command path no
longer merely creates a lease and that synthetic `"mock-adapter"` process
success is not present in the production dispatch path.

Do not start P8 until these gates and the existing P7 gates pass.
