# Relintor Milestone 8 — Independent Source Audit

## Verdict

`MILESTONE_8_REPAIR_REQUIRED`

The P8 crate contains useful verification primitives and the 43-test corpus is
not empty, but the production source does not yet support the claimed P8
milestone authority.

This is a P8-only repair. Do not reopen P7 and do not start P9.

## P8-IA-01 — P8 is not wired into the shipped desktop product

`apps/desktop/src-tauri/Cargo.toml` has no dependency on `relintor-evidence`.
The Tauri command surface exposes P1–P7 operations but no P8 verification,
evidence, manifest, or certificate command. The React/backend surface contains
no P8 project verification workflow.

Therefore the desktop/native PASS is a regression build, not proof that a user
can actually run P8.

Repair:
- add the P8 Rust authority to the Tauri product path;
- load the sealed P6 authority and authenticated P7 execution result;
- expose Rust-owned verification start/status/re-run/evidence/certificate
  commands;
- add the project Verification UX without making React authoritative;
- keep keys and evidence authority out of the renderer.

## P8-IA-02 — Caller-supplied metadata can manufacture strong PASS evidence

`EvidenceStore::put` accepts caller-created `EvidenceMetadata`. The caller
chooses:
- evidence class;
- requirement links;
- PASS/FAIL;
- confidence including `STRONG_DETERMINISTIC`;
- accepted acceptance-criterion IDs;
- collector identity.

The HMAC authenticates the bytes/metadata after insertion, but it does not prove
that the metadata came from a real collector.

The acceptance corpus itself creates strong PASS evidence by constructing
metadata and storing `b"test passed"`.

Repair:
- separate raw blob storage from evidence authority;
- production evidence must be created from Rust-owned collector receipts;
- result/confidence/class/criterion satisfaction must be derived by trusted
  collector/verification logic, not supplied by the builder/renderer;
- test fixtures may have a clearly test-only receipt minting path;
- arbitrary caller-created metadata must never satisfy a production obligation.

## P8-IA-03 — A forged VerificationReport can be upgraded and certified

`VerificationReport` is mutable/public data and `CompletionAuthority::issue`
accepts it as authority. `issue` checks the final state and authority digest but
does not prove that the report was actually emitted by `VerificationEngine`.

A caller can take an incomplete report, change its decision to
`VERIFIED_COMPLETE`, and request a valid HMAC certificate.

Repair:
- bind VerificationReport to a Rust-owned integrity/issuance proof;
- or make completion evaluation + certificate issuance one internal authority
  transaction;
- `CompletionAuthority::issue` must revalidate report integrity and manifest
  state, not trust public struct fields;
- a caller-mutated report must be rejected.

## P8-IA-04 — P8 does not authenticate the real P7 execution ledger

`VerificationAuthority` accepts string fields:
- `p7_run_id`
- `p7_state`
- `workspace_fingerprint`

and only checks that the state string equals
`EXECUTION_TASKS_FINISHED_AWAITING_VERIFICATION`.

`relintor-evidence` does not depend on `relintor-execution`, so P8 cannot verify
the authenticated P7 ledger/snapshot or its actual run/task outcomes.

Repair:
- P8 admission must consume a validated P7 execution snapshot/receipt from the
  P7 Rust authority;
- verify ledger integrity, mission/revision/run identity, terminal state and
  relevant task outcomes;
- a caller-created string must not stand in for P7 completion authority.

## P8-IA-05 — Invalidation tombstones are unauthenticated and deletable

Evidence metadata is HMAC-protected, but `EvidenceInvalidation` is written as
plain JSON. Freshness treats the mere presence of the invalidation file as the
revocation.

Deleting the tombstone resurrects the old evidence.

Repair:
- authenticate invalidation records;
- maintain a signed/authenticated invalidation index/chain or equivalent durable
  monotonic authority so deletion is detectable;
- a previously invalidated evidence ID must not become Fresh merely because one
  local tombstone file disappeared;
- P9 may later own recovery; P8 must at least fail closed on tamper/deletion.

## P8-IA-06 — H-05 through H-09 are observation DTOs, not collectors

The source contains:
- `ApiDatabaseObservation`
- `BrowserRuntimeObservation`
- `ScreenshotObservation`
- `AccessibilityObservation`
- `PerformanceObservation`
- `SecurityObservation`

but no corresponding production collectors. The acceptance tests simply
construct these structs with `result: PASS/FAIL` and assert that the field still
contains that value.

That does not prove API/database/browser/accessibility/performance/security
collection.

Repair:
- implement real bounded collector/adaptor boundaries;
- production results must come from executed probes/tools/runtime;
- test adapters may be injected;
- if a real runtime is unavailable, keep live smoke PENDING rather than
  fabricating an observation.

## P8-IA-07 — Independent AI verifier is disconnected from completion authority

`IndependentAiVerifier` is only a generic trait wrapper. There is no production
server-side gateway adapter in this crate/product path.

More importantly:
- `VerificationEngine::evaluate` does not consume AI judgements;
- both manifest construction paths hard-code `ai_judgements: Vec::new()`;
- the "deterministic failure versus AI supported" test runs the AI verifier and
  completion engine separately rather than testing one integrated decision.

Repair:
- add the real server-side AI gateway adapter boundary;
- keep the live provider status pending when credentials/runtime are absent;
- store AI judgement as authenticated evidence/reference;
- feed validated AI judgements into the verification policy only where required;
- deterministic failure must still win in the same integrated completion path;
- manifest must export the actual judgement references.

## P8-IA-08 — Non-machine-checkable acceptance criteria can disappear

The engine checks only:

`acceptance_criteria.filter(|criterion| criterion.machine_checkable)`

A sealed non-machine-checkable criterion therefore has no completion obligation
and can silently disappear from coverage.

Repair:
- every acceptance criterion must be explicitly accounted;
- machine-checkable criteria require deterministic/runtime evidence;
- non-machine criteria require the allowed AI/human/explicit evidence policy;
- missing/inconclusive non-machine criteria must block VERIFIED_COMPLETE.

## P8-IA-09 — N/A/deferred states incorrectly imply accepted-risk completion

The completion branch returns `COMPLETE_WITH_ACCEPTED_RISKS` whenever any
requirement is `NOT_APPLICABLE` or `DEFERRED_BY_EXPLICIT_DECISION`, even when
`accepted_risks` is empty.

`NOT_APPLICABLE` is not itself an accepted risk.

Repair:
- N/A remains accounted but does not manufacture an accepted-risk outcome;
- explicit deferment must follow its sealed decision/risk policy;
- `COMPLETE_WITH_ACCEPTED_RISKS` requires actual authorized risk records;
- all applicable verified requirements plus legitimate N/A accounting may still
  reach the appropriate normal completion state.

## P8-IA-10 — Impact-based invalidation is not the actual freshness path

`impacted_by_change` can say an unrelated path is not relevant, but
`freshness` requires exact equality of the whole workspace fingerprint and
source revision. Any normal source-state change therefore stales every evidence
item before the impact result matters.

Repair:
- integrate impact/scoped source identity into freshness;
- relevant source/lock/authority changes invalidate affected evidence;
- provably unrelated changes may preserve unaffected evidence;
- unknown impact remains conservative.

## Closure requirements

Do not start P9 until:
- the included Rust source-audit tests pass;
- the static product/source audit passes;
- the original 43 P8 acceptance tests remain green with stronger fixtures;
- workspace/desktop/native/security regressions remain green;
- H/I traceability is reconciled to actual product behavior, not DTO presence.
