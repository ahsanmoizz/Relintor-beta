# Relintor Milestone 8 — Last Authority/Product Closure Verification

## Current verdict

MILESTONE_8_LOCAL_VERIFIED_CROSS_PLATFORM_PENDING

This is the single canonical current state after the P8-LC-01 through P8-LC-04,
P8-CC-01/P8-CC-02, P8-CC-E2E-01/P8-CC-E2E-02, and P8-ZC-01/P8-ZC-02
repairs. P9 is NOT STARTED.
GitHub was not touched. P7 was not reopened and P8 was not broadened.

Windows local gates pass. macOS and Linux hosted gates have not run and remain
deferred.

## Scope and preservation

Repository: D:\Relintor

The following were preserved:

- spec/locked
- pnpm-lock.yaml
- all carried P2/P3 debt

All Rust, Cargo, target, temporary, pnpm-store, and Gitleaks paths used for the
verification were D:-first:

- RUSTUP_HOME=D:\Relintor-rustup
- CARGO_HOME=D:\Relintor-cargo-home
- CARGO_TARGET_DIR=D:\Relintor\target
- TEMP/TMP=D:\Relintor-temp
- pnpm store: D:\Relintor-pnpm-store
- Rust: 1.96.0-x86_64-pc-windows-msvc
- MSVC environment: D:\DevTools\VSBuildTools\Common7\Tools\VsDevCmd.bat

## Final product/authority closure

P8-FC-01 — PASS. Added VerificationCollectorOrchestrator. Desktop
verification_start and verification_rerun derive required work from the sealed
P6 requirement/evidence plan, reuse only fresh matching evidence, execute
applicable collector boundaries, persist trusted receipts, and leave unavailable
live dependencies incomplete.

P8-FC-02 — PASS. Removed public production PASS-fixture constructors and
EvidenceStore fixture insertion. Fixture support is isolated behind the
test-support feature, whose package default is now `default = []`; the desktop
dependency also disables default features. The self dev-dependency explicitly
enables test-support only for integration-test compilation.

P8-FC-03 — PASS. VerificationReport authentication is now HMAC-backed by the
Rust-owned evidence authority key and binds the report run, sealed authority,
P7 ledger digest, requirement ledger, deterministic decisions, manifest digest,
and AI judgement references. Caller-recomputed public SHA-256 checksums are
rejected.

P8-FC-04 — PASS. The weak constructor and certificate route are test-support
only. Production construction requires AuthenticatedP7Execution and production
certificate issuance uses issue_with_p7_execution.

P8-FC-05 — PASS. CollectorBinding validates requirement and criterion
membership against the sealed plan. for_requirement validates evidence class,
required obligation, and sealed criteria before a collector can mint a receipt.

P8-FC-06 — PASS. Added real BuildCollector, TestCollector, and
StaticAnalysisCollector boundaries. Build receipts include real process exit
status, bounded output, tool identity, and artifact hashes. Test receipts
include parsed inventory, passed/failed/skipped/ignored/not-discovered counts,
and source hashes. Static-analysis receipts include real process result,
bounded output, and tool identity. H-05 through H-09 acceptance now executes
collector boundaries with deterministic adapters/processes rather than
constructing PASS DTOs.

P8-FC-07 — PASS for production capability. relintor-evidence now depends on the
existing relintor-ai-gateway and ProductionAiProvider sends authenticated
server-side gateway requests through its client boundary. No renderer key is
used. Live independent AI execution remains pending because no live provider
was configured:

P8_REAL_INDEPENDENT_AI_VERIFIER=PENDING_EXTERNAL_ENVIRONMENT

The integrated engine still gives a deterministic gate failure precedence over
an AI-supported judgement.

P8-FC-08 — PASS. The desktop resolves the authority secret through the core
OS-keychain boundary. No raw p8-local-authority.key app-data path remains in
the production evidence or Tauri authority path. The renderer never receives
the key.

P8-FC-09 — PASS. Deferred completion now requires per-requirement matching
authorized human/user risk accounting. N/A is not treated as a risk; unrelated
risk records do not authorize another deferred requirement; non-waivable
requirements cannot be risk-accepted.

## Baseline evidence

The required baseline Rust last-authority closure command exited 0:
1 passed, 0 failed, 0 ignored. The supplied criterion-provenance test already
passed before repair.

The required baseline static closure command exited 1 and reported the seven
P8-LC findings applicable to the pre-repair source: LC-01, two LC-02 findings,
LC-03, and three LC-04 findings. These are superseded by the passing final
results below.

The first post-feature `cargo test ... --locked` attempt exited 101 because the
new explicit test-support dev dependency required Cargo.lock regeneration. The
lockfile was then regenerated and all locked gates passed.

## P8 Last Authority Closure Reconciliation

P8-LC-01 — PASS. `crates/relintor-evidence/Cargo.toml` now has
`default = []` and `test-support = []`. Production dependencies do not receive
fixture evidence support by default.

P8-LC-02 — PASS. `VerificationCollectorPlan::discover` is Rust-owned and
derives executable verification commands from discovered Cargo, package.json
scripts/package manager, Python project configuration, Gradle wrappers, and an
explicit `.relintor/verification-plan.json` project configuration. The
orchestrator routes BuildOutput, TestOutput, LintStaticAnalysis, ApiResponse,
DatabaseQuery, BrowserRecording, Screenshot, AccessibilityResult, and
SecurityScan through their collector boundaries. Missing configured boundaries
remain blocked/pending; no synthetic PASS is produced. Cargo is not executed for
a non-Cargo project when a discovered non-Cargo plan is present.
The 47-test P8 acceptance corpus exercises Node/non-Cargo planning, configured
API/browser/security entries, and missing-collector truthfulness.

P8-LC-03 — PASS. `CollectorBinding::for_requirement` maps each criterion to
the evidence class whose criterion type it actually covers. BuildOutput cannot
claim runtime/browser/API/security criteria. The independent build-versus-runtime
test passed: 1/1.

P8-LC-04 — PASS. `parse_ai_gateway_response` requires a structured JSON object
with status, reasoning_summary, evidence_ids, confidence, and identified_gaps;
malformed, unrecognized, unbound, or contradictory responses become
INCONCLUSIVE and never SUPPORTED. The desktop constructs minimized AI input,
calls the server-side provider when configured, binds returned judgement
references to the same VerificationEngine run, and exports them in the
authenticated EvidenceManifest. Deterministic failure remains dominant over
AI SUPPORTED in the integrated acceptance flow.

AI response outcomes: structured parser PASS; malformed response INCONCLUSIVE;
UNSUPPORTED preserved; INCONCLUSIVE preserved; live provider
`P8_REAL_INDEPENDENT_AI_VERIFIER=PENDING_EXTERNAL_ENVIRONMENT`.

## P8 Final Certification Guard Reconciliation

P8-CC-01 — PASS. Generic `CollectorBinding::for_requirement` now claims no
acceptance criteria. Criterion claims require `for_criterion` with an exact
sealed requirement/criterion, evidence class, collector identity, and executed
probe identity. A generic same-class test cannot prove unrelated criteria.

P8-CC-02 — PASS. Raw `AiVerifierJudgement` injection is test-support-only.
Production uses `AuthenticatedAiJudgement`, HMAC-bound to mission, revision,
P6 seal, P7 execution identity, minimized-input digest, evidence IDs/digests,
provider identity, structured judgement, response digest, timestamp, and P8
authority identity. Desktop now follows provider → parser → authenticated
receipt → VerificationEngine. Deterministic FAIL precedence is preserved.

Criterion-specific provenance test: PASS — 1 passed, 0 failed.
Final certification static guard: PASS.
Raw AI judgement production API: restricted to test-support; no public raw
`with_ai_judgements` production authority input.
Authenticated AI judgement authority: PASS in the production engine path.

## P8 CC End-to-End Reconciliation

P8-CC-E2E-01 — PASS. The production verification plan now accepts explicit,
trusted criterion-to-probe mappings. When a mapped probe executes, the
orchestrator calls `CollectorBinding::for_criterion` with the exact sealed
requirement, criterion, evidence class, collector identity, and probe identity.
Discovered class-level commands have no criterion mapping and remain
criterion-neutral; they cannot guess or claim acceptance criteria. Collector
receipt provenance is integrity-bound to the actual collector and command.

Runtime/human criterion satisfaction now requires the artifact to carry the
criterion ID in `accepted_criteria`. The supplied unrelated-runtime test and
the existing build-versus-runtime test both reject unrelated PASS evidence.
The product orchestration test proves a trusted mapping mints criterion-bound
evidence and executes the mapped probe.

P8-CC-E2E-02 — PASS. `AiProviderResult` is now an opaque production receipt:
its judgement, provider identity, and response digest are private and can only
be constructed by the in-module `ProductionAiProvider` adapter. The production
path remains provider → structured gateway parser → opaque provider receipt →
P8 authentication → `VerificationEngine`. A caller-created provider result
cannot be constructed through the public API; authentication remains bound to
mission, revision, P6 seal, P7 execution, requirement, minimized-input digest,
evidence digests, provider identity, response digest, timestamp, and P8
authority. Deterministic FAIL precedence remains intact.

End-to-end Rust guard: PASS — 1 passed, 0 failed.
End-to-end static guard: PASS.
Criterion-specific production orchestration: PASS — 1 product test passed.
Unrelated runtime evidence rejection: PASS — 1 supplied guard test passed.
Opaque/provider-authenticated AI result: PASS.
Fabricated `AiProviderResult` rejection: PASS by opaque public API; no public
authority-bearing fields or production constructor are exposed.

## P8 ZC Final Authority Reconciliation

P8-ZC-01 — PASS. `.relintor/verification-plan.json` is now candidate tooling
configuration only. Its commands, arguments, routes, and scanners are
discovered for execution, but its criterion claims are ignored as authority.
Production orchestration starts with an empty protected criterion plan unless
an authenticated Rust-owned protected plan is supplied.

Protected criterion authority binds mission ID, mission revision, project ID,
P6 seal hash, requirement ID, criterion ID, evidence class, collector identity,
probe identity, candidate command digest, and protected verification-plan
authority digest. The collector receipt integrity proof carries the same
provenance.

Mutable workspace mapping attack: PASS — a workspace `sensitive-behavior`
claim with an always-successful command did not verify the sealed criterion.
Authorized criterion mapping: PASS — exact protected mapping executed and
produced criterion-bound evidence. Candidate command/probe digest mismatch:
PASS — criterion remained unverified. Requirement/criterion mismatch: PASS —
rejected. Criterion/evidence-class mismatch: PASS — rejected. Generic evidence
still satisfies legitimate evidence obligations but cannot satisfy a
criterion-specific acceptance proof.

P8-ZC-02 — PASS. `AiProviderResult` is genuinely opaque in production: its
authority fields are private and it no longer derives `Serialize` or
`Deserialize`. The production path is provider → real gateway response →
strict structured parser → opaque provider result → P8 authentication →
`AuthenticatedAiJudgement` → `VerificationEngine`.

Trusted provider to authenticated judgement: PASS in the production adapter
path. Fabricated provider result: PASS — no public field or deserialization
construction route exists. Tampered response digest and wrong mission,
revision, P6, P7, requirement, input, evidence, provider, or authority
bindings are rejected by the authenticated envelope checks. Deterministic FAIL
continues to take precedence over authenticated AI SUPPORTED.

Absolute-final Rust guard: PASS — 2 passed, 0 failed.
Absolute-final static guard: PASS.

## Final command ledger

Every command below ran on Windows in the x64 MSVC developer environment.
Unless stated otherwise, exit code 0 is PASS.

| Exact command | Exit code | Result |
|---|---:|---|
| cargo fmt --all | 0 | PASS |
| cargo generate-lockfile | 0 | PASS — regenerated from the repaired workspace |
| cargo fmt --all -- --check | 0 | PASS |
| cargo clippy --workspace --all-targets --locked -- -D warnings | 0 | PASS |
| cargo test -p relintor-evidence --test independent_p8_final_certification_guard --locked -- --nocapture | 0 | PASS — 1 passed, 0 failed, 0 ignored |
| cargo test -p relintor-evidence --test independent_p8_absolute_final_authority_guard --locked -- --nocapture | 0 | PASS — 2 passed, 0 failed, 0 ignored |
| cargo test -p relintor-evidence --test independent_p8_cc_end_to_end_guard --locked -- --nocapture | 0 | PASS — 1 passed, 0 failed, 0 ignored |
| cargo test -p relintor-evidence --test independent_p8_final_product_closure --locked -- --nocapture | 0 | PASS — 2 passed, 0 failed, 0 ignored |
| cargo test -p relintor-evidence --test independent_p8_last_authority_closure --locked -- --nocapture | 0 | PASS — 1 passed, 0 failed, 0 ignored |
| cargo test -p relintor-evidence --test independent_p8_source_audit --locked -- --nocapture | 0 | PASS — 3 passed, 0 failed, 0 ignored |
| cargo test -p relintor-evidence --test independent_p8_acceptance --locked -- --nocapture | 0 | PASS — 47 passed, 0 failed, 0 ignored; includes protected criterion authority and mismatch tests |
| python tooling/acceptance/independent_p8_absolute_final_authority_guard.py | 0 | PASS |
| python tooling/acceptance/independent_p8_cc_end_to_end_guard.py | 0 | PASS |
| python tooling/acceptance/independent_p8_final_certification_guard.py | 0 | PASS |
| python tooling/acceptance/independent_p8_final_product_closure.py | 0 | PASS |
| python tooling/acceptance/independent_p8_last_authority_closure.py | 0 | PASS |
| python tooling/acceptance/independent_p8_source_audit.py | 0 | PASS |
| cargo test --workspace --locked | 0 | PASS — 206 passed, 0 failed, 1 ignored |
| python tooling/spec/verify_spec.py | 0 | PASS — 18 files, 17 manifest entries, 144 features |
| python tooling/acceptance/test_spec_verifier.py | 0 | PASS — baseline PASS; all seven negative cases FAIL_AS_EXPECTED |
| python tooling/acceptance/verify_traceability.py | 0 | PASS — 144 records, 144 unique IDs |
| python tooling/acceptance/secret_scan.py | 0 | PASS |
| pnpm install --frozen-lockfile --store-dir D:\Relintor-pnpm-store | 0 | PASS |
| pnpm --dir apps/desktop typecheck | 0 | PASS |
| pnpm --dir apps/desktop lint | 0 | PASS |
| pnpm --dir apps/desktop test | 0 | PASS — 1 file, 6 tests |
| pnpm --dir apps/desktop build | 0 | PASS |
| pnpm --dir apps/desktop tauri build --no-bundle, inside VsDevCmd x64 with D:-first MSVC variables | 0 | PASS — cl.exe D:\DevTools\VSBuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\cl.exe; link.exe at the matching MSVC path; rustc host x86_64-pc-windows-msvc; built D:\Relintor\target\release\relintor-desktop.exe |

The desktop test output contained React act(...) warnings only; no test failed.

## Collector and authority test totals

- final certification guard: 1 passed, 0 failed
- final product closure: 2 passed, 0 failed
- last authority closure: 1 passed, 0 failed
- prior independent P8 source audit: 3 passed, 0 failed
- strengthened P8 acceptance: 47 passed, 0 failed (includes protected criterion authority and mismatch tests)
- P8 CC end-to-end guard: 1 passed, 0 failed
- P8 absolute-final authority guard: 2 passed, 0 failed
- full workspace: 206 passed, 0 failed, 1 ignored
- live PostgreSQL integration: the existing cloud-api test remains ignored unless
  RELINTOR_TEST_DATABASE_URL is explicitly supplied

## Static closure result

python tooling/acceptance/independent_p8_final_certification_guard.py:
PASS

python tooling/acceptance/independent_p8_absolute_final_authority_guard.py:
PASS

python tooling/acceptance/independent_p8_cc_end_to_end_guard.py:
PASS

python tooling/acceptance/independent_p8_final_product_closure.py:
PASS

python tooling/acceptance/independent_p8_source_audit.py:
PASS

## Security and Gitleaks

Gitleaks executable:
D:\Relintor-cargo-home\gitleaks-tools\gitleaks-8.30.1\gitleaks.exe

Exact per-scope command:

D:\Relintor-cargo-home\gitleaks-tools\gitleaks-8.30.1\gitleaks.exe dir --redact --no-banner --exit-code 1 SCOPE

All 13 scopes exited 0 and reported no leaks:

| Scope | Exit code | Result |
|---|---:|---|
| .github | 0 | PASS |
| apps/desktop | 0 | PASS |
| crates | 0 | PASS |
| db | 0 | PASS |
| integrations | 0 | PASS |
| packages | 0 | PASS |
| services | 0 | PASS |
| tooling | 0 | PASS |
| Cargo.toml | 0 | PASS |
| package.json | 0 | PASS |
| pnpm-workspace.yaml | 0 | PASS |
| rust-toolchain.toml | 0 | PASS |
| REPAIR_MANIFEST.json | 0 | PASS |

## Traceability

Traceability verification is PASS — 144 records and 144 unique IDs.

H-01 through H-12: implementation_status=implemented and
verification_status=passed.

I-01 through I-12: implementation_status=implemented and
verification_status=passed.

H-02, H-03, H-04, H-05, H-06, H-07, H-08, H-09, H-11, and H-12 were
revalidated against the real collector/orchestration and authority paths.
I-08, I-10, I-11, and I-12 were revalidated against the repaired
per-requirement risk, completion-authority, certificate, and manifest paths.

J-series remains NOT STARTED.

## Locks and sealed specification

Cargo.lock was regenerated from the repaired workspace.

- Cargo.lock SHA-256: 9C70278C3A026A3AD6945C2D886DAC4E5D47AC67C8274762DAA5D19392F29D9A
- pnpm-lock.yaml SHA-256: 09FAF049676145597CDEEA7C15FA20772D8C0C264CD40B319FFA9BC45B4CC2AC
- spec/locked manifest SHA-256: B0A05C46B28D2E9DFE27233394FFD308E2F75302C57CE4B619BC9A6198D2BE78

Spec verification reports:

SPEC_VERIFY_PASS files=18 manifest_entries=17 features=144 unique_ids=144

spec/locked was not modified. pnpm-lock.yaml was not modified.

## Platform matrix

| Platform | Current status | Evidence |
|---|---|---|
| Windows | PASS — local P8 gate | x64 MSVC fmt, clippy, workspace tests, closure/source/acceptance tests, Python gates, desktop gates, native Tauri, and Gitleaks |
| macOS | NOT_RUN_CROSS_PLATFORM_GATE_DEFERRED | No hosted runner executed |
| Linux | NOT_RUN_CROSS_PLATFORM_GATE_DEFERRED | No hosted runner executed |

## Carried external debt and remaining work

- P2_LIVE_POSTGRES_INTEGRATION=PENDING_EXTERNAL_ENVIRONMENT
- P3_REAL_ANTIGRAVITY_SMOKE=PENDING_EXTERNAL_ENVIRONMENT
- P3_PLUGIN_SIGNING_PACKAGING=DEFERRED_TO_RELEASE_GATE
- P3_INDEPENDENT_SOURCE_AUDIT=NOT_RUN
- P7_REAL_ANTIGRAVITY_END_TO_END=PENDING_EXTERNAL_ENVIRONMENT
- P8_REAL_INDEPENDENT_AI_VERIFIER=PENDING_EXTERNAL_ENVIRONMENT
- P8_REAL_BROWSER_RUNTIME_SMOKE=PENDING_EXTERNAL_ENVIRONMENT
- macOS/Linux cross-platform gates: NOT_RUN
- P9: NOT STARTED

Exact remaining blocker for full cross-platform certification: hosted macOS and
Linux required gates have not executed. The live AI/browser/PostgreSQL items
remain external-environment evidence gaps and are not represented as synthetic
PASS evidence.

## Storage

Measured after final testing:

- C: 610,230,272 bytes free
- D: 58,734,419,968 bytes free

C: remains critically low. No user/system files were deleted. All new Rust
dependencies, build output, temporary data, pnpm store, and Gitleaks tooling
were kept on D: where configured.

## Historical attempts / superseded evidence

The baseline static audit failed with the seven P8-LC findings listed above.
Those findings are superseded by the passing final closure/source/acceptance
runs. The baseline Rust criterion test itself passed.

One initial unqualified pnpm Tauri invocation inherited the GNU toolchain and
failed before build execution while rustup attempted to recover the GNU
toolchain from C: with OS error 112. It is not the current native result. The
required x64 MSVC invocation with RUSTUP_TOOLCHAIN=1.96.0-x86_64-pc-windows-msvc
then passed and produced the native executable listed above.

A first PowerShell quoting attempt for the required batch environment also
exited 1 before the developer environment ran, with raw error:
`'\\' is not recognized as an internal or external command`. The corrected
`cmd /c` invocation ran `VsDevCmd.bat -arch=x64 -host_arch=x64` and passed.

## Files changed for this final certification delta

- crates/relintor-evidence/src/lib.rs
- crates/relintor-evidence/tests/independent_p8_acceptance.rs
- crates/relintor-evidence/tests/independent_p8_absolute_final_authority_guard.rs
- tooling/acceptance/independent_p8_absolute_final_authority_guard.py
- tooling/evidence/milestone-8-verification.md

Cargo.lock was not changed by this delta; its prior regenerated hash remains
authoritative.

No GitHub files or remote state were changed.

STOP AFTER P8.
