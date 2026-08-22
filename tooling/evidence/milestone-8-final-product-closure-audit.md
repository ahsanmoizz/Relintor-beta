# Relintor Milestone 8 — Final Product/Authority Closure Audit

## Verdict

`MILESTONE_8_FINAL_PRODUCT_REPAIR_REQUIRED`

The IA-01..IA-10 repair materially improved P8, but the certification delta
still contains production-authority gaps. This is a narrow final P8 closure;
do not reopen P7 and do not start P9.

## P8-FC-01 — Desktop "verification_start" evaluates an empty/existing store but does not collect evidence

The desktop now exposes P8 commands, but `evaluate_p8` constructs
`VerificationEngine::new_with_p7_execution(..., Vec::new(), ...)` and immediately
calls `evaluate(None)`. It does not run build/test/static/API/browser/etc.
collectors or a verification-plan orchestrator.

A user can open Verification, but there is no product path that actually
collects the required evidence.

Repair:
- add a Rust-owned `VerificationCollectorOrchestrator` (or equivalent);
- derive required collectors from the sealed requirement/evidence plan;
- `verification_start`/`verification_rerun` execute the applicable collectors
  before evaluation;
- existing fresh evidence may be reused only after freshness/impact validation;
- unavailable live dependencies remain BLOCKED/PENDING, never synthetic PASS.

## P8-FC-02 — "test-only" PASS minting APIs are public in production

`CollectorReceipt::test_fixture` and `EvidenceStore::put_test_fixture` are
`pub`. `#[doc(hidden)]` is documentation-only; another production Rust crate can
still call them and mint arbitrary PASS/STRONG_DETERMINISTIC metadata.

Repair:
- remove these APIs from the normal production library surface;
- move fixtures into test support compiled only for tests/dev feature that the
  production desktop does not enable;
- no production dependent crate may mint fixture evidence.

## P8-FC-03 — VerificationReport integrity is an unkeyed checksum, not authority authentication

`VerificationReport::validate_integrity` recomputes plain SHA-256 over public
report fields. Any caller that can alter those fields can recompute the same
checksum.

`CompletionAuthority::issue` then accepts a report marked VERIFIED_COMPLETE.

Repair:
- authenticate reports with a Rust-owned secret/authority key (HMAC/signature),
  or make evaluation + certificate issuance one internal transaction;
- the certificate authority must not accept a caller-recomputed public checksum;
- report proof must bind the authenticated P7 execution and evidence manifest.

## P8-FC-04 — Weaker non-P7 completion APIs remain public

`VerificationEngine::new` permits an engine with no authenticated P7 execution,
and `CompletionAuthority::issue` issues without requiring
`AuthenticatedP7Execution`.

The desktop uses the stronger variants, but the production library still
publishes weaker authority routes.

Repair:
- make non-P7 constructors/issuance private, crate-private, or test-only;
- production completion always requires authenticated P7 execution;
- keep dedicated test fixture helpers outside the production API.

## P8-FC-05 — Collector bindings are caller-selected rather than sealed-plan-bound

`CollectorBinding::from_authority` accepts caller-supplied requirement IDs and
accepted criterion IDs without validating that they exist or belong together.

This means a real successful probe can be attached to an unrelated requirement
or criterion.

Repair:
- derive collector binding from the sealed requirement + verification policy;
- reject unknown/mismatched requirement IDs and criterion IDs;
- bind collector class and probe/command family to the required evidence
  obligation;
- renderer/builder must not choose which criterion a successful command proves.

The included Rust test checks the first fail-closed boundary.

## P8-FC-06 — H-02/H-03/H-04 still lack production collector types; H-05..H-09 acceptance remains shallow

The source has generic `ProcessCollector` plus H-05..H-09 wrappers, but no
production:
- BuildCollector
- TestCollector
- StaticAnalysisCollector

The 43-test acceptance file still contains shallow tests that directly construct
`ApiDatabaseObservation`, `BrowserRuntimeObservation`,
`AccessibilityObservation`, `PerformanceObservation`, and
`SecurityObservation` with PASS/FAIL values.

Repair:
- add real Build/Test/StaticAnalysis collector wrappers that mint receipts from
  actual process results;
- strengthen the shallow H-05..H-09 tests to execute deterministic test
  adapters/processes through the collectors;
- `TestEvidenceSummary` must be populated from actual test-process/parser
  output, not caller totals.

## P8-FC-07 — Production AI provider is a stub, not a gateway integration

`ProductionAiProvider::judge` returns PENDING/Err even when configured.
`relintor-evidence` has no dependency on the existing `relintor-ai-gateway`
crate and the desktop evaluates with `Vec::new()` AI judgements.

Repair:
- integrate the real existing server-side AI gateway abstraction;
- live provider may remain PENDING when credentials/network are unavailable;
- when policy requires AI review, the production orchestrator actually calls
  the gateway adapter and feeds authenticated judgement references into the
  engine/manifest;
- deterministic failure still wins in the same integrated path.

## P8-FC-08 — Completion key is stored as a raw app-data file

`load_or_create_secure_local_authority_key` writes
`p8-local-authority.key` as raw bytes in app data and notes that a service "may"
replace it with a keychain provider.

Because this key authenticates evidence/invalidation/certificates, production
must use the existing secure OS-keychain/local secret authority rather than a
replaceable plaintext file.

Repair:
- use the existing OS keychain authority from the desktop/core layer;
- keep filesystem fallback test-only or explicitly degraded/fail-closed;
- renderer never receives the key.

## P8-FC-09 — Accepted-risk completion is not matched per deferred deviation

The decision branch returns `COMPLETE_WITH_ACCEPTED_RISKS` whenever any accepted
risk exists, before checking whether every deferred requirement has its own
authorized risk/defer accounting.

One risk record must not cover an unrelated deferred requirement.

Repair:
- require per-requirement matching between each remaining deviation/defer and
  its valid accepted-risk/explicit decision;
- N/A remains non-risk;
- a deferred requirement with no matching allowed risk keeps completion
  incomplete.

Also replace the current no-op acceptance test
`not_applicable_and_deferred_states_are_explicitly_accounted` with an actual
engine scenario.

## Closure rule

P8 may be certified only when:
- the two included Rust authority tests pass;
- the final static product audit passes;
- P8 acceptance is strengthened and remains green;
- full workspace/desktop/native/security gates remain green;
- H/I traceability reflects actual user-runnable product behavior.
