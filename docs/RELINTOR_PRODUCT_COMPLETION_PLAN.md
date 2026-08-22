# Relintor Product Completion Plan

**Document purpose:** executable roadmap from the current Windows x64 near-Beta
state to a production-quality, multi-IDE Relintor product. This is a planning
document, not implementation authority. It must never be used to convert an
unverified item into a product claim.

**Snapshot:** 2026-08-18, repository `D:\Relintor`.

**Evidence labels used in this plan:**

- `VERIFIED` — current repository or executed evidence supports the claim.
- `IMPLEMENTED_UNVERIFIED` — source/tests exist, but the required real runtime,
  release, or clean-machine evidence has not run.
- `BLOCKED_ENVIRONMENT` — execution needs a missing toolchain, service, runner,
  credential, device, or other external dependency.
- `PLANNED` — agreed engineering work not yet implemented.
- `REQUIRES PRODUCT DECISION` — an engineering AI must not choose the product
  policy silently.
- `FUTURE` — intentionally outside the current release scope.

## 1. Current Starting Point

### Repository-verified state

The repository contains three application surfaces (`apps/desktop`,
`apps/admin`, `apps/website`), a twelve-member Rust workspace, nine reusable
Rust crates, two services, three package directories, ten database migrations,
and the locked specification and traceability tooling. The exact current
inventory and architecture are recorded in
[`RELINTOR_REPOSITORY_VERIFICATION.md`](RELINTOR_REPOSITORY_VERIFICATION.md).

### Completed or source-backed

- Local-first desktop authority, SQLite project/evidence state, native command
  boundary, execution/recovery, investigator/takeover, standards and evidence
  crates are present in the repository.
- Cloud API route families, PostgreSQL store, memory test store, session
  rotation, tenant binding, entitlement/grant foundations, admin/MFA routes and
  Google OIDC validation are source-backed.
- The AI gateway has an explicit provider boundary, server-side credential
  names, bounded I/O, timeout handling, usage accounting and fail-closed mock
  selection. A live provider call is not proven.
- The active Tauri updater configuration is a non-null inert object. Empty
  production updater values remain unavailable; no fake key or endpoint is
  present.
- Source-level P1–P12 audit/repair work and local deterministic acceptance
  suites are present. This does not mean that every external release gate has
  passed.
- Current frontend verification recorded in the repository audit: desktop
  typecheck, lint and 10 tests pass; website typecheck, lint and 2 tests pass.
- Specification, traceability and secret-scan gates pass in the current audit.

### Verified artifacts

- Current Windows x64 installer:
  `target/release/bundle/nsis/Relintor_0.1.0_x64-setup.exe`
- Installer SHA-256:
  `95CD461A9D9EDF5598A1A406B257B373B0B24FA195F0A8046B27ED618EABC250`
- Current installer last-write time:
  `2026-08-18T22:55:58.6358574+05:00`
- Current native executable SHA-256:
  `A21DEFF2360FEEBD182C93E9D7B1EA729DBCFBDA24E28136044A4A6C5BB24AB9`
- `apps/website/public/downloads/Relintor_0.1.0_x64-setup.exe` matches the
  current installer.

### Incomplete or blocked

- The old updater null-configuration panic is historically recorded as fixed,
  but a native regression test and an explicit repaired-process exit code are
  still missing.
- `apps/website/dist/downloads/Relintor_0.1.0_x64-setup.exe` is stale and has
  the older SHA-256 `9644DEDA0C114A684E9A2B45A339252520FB433C74802B03BA20583FDD7D7199`.
- The current runner has no installed Rust toolchain; current Rust fmt/clippy/
  workspace-test evidence is therefore `BLOCKED_ENVIRONMENT`.
- Clean-machine installation, launch, upgrade, uninstall, signing, rollback,
  live PostgreSQL, live Google, live AI provider, live Antigravity, hosted
  macOS/Linux, ARM64, production domains, observability and release operations
  are not currently certified.
- `PROJECT_CONTEXT_TRANSFER.md` was not found as a filesystem file in the
  repository or attachment search. Its historical intent is not implementation
  evidence and must be reconciled if a copy is later restored.

### Current release status

`IMPLEMENTED / WINDOWS X64 ARTIFACT PRESENT / BETA DEPLOYMENT NOT CLOSED`

### Current exact next engineering task

`REL-TASK-001` — add and run the updater configuration regression test proving
that missing production updater provisioning cannot crash startup or report a
false update success.

## 2. Product Completion Definition

### Beta complete

Relintor Beta is complete only when all of the following are evidenced for the
supported Windows x64 target:

1. The pinned MSVC fmt, clippy, workspace tests, native no-bundle build and
   desktop/web quality gates pass on the release source and lockfiles.
2. The repaired executable launches with a captured exit code and no startup
   panic; the updater is explicitly unavailable until real signed configuration
   is installed.
3. A clean disposable Windows machine installs, launches, authenticates,
   discovers the supported Antigravity runtime, completes a bounded project
   journey, exports evidence, and uninstalls according to the documented data
   policy.
4. Real external gates are separately evidenced: PostgreSQL migrations and
   account/session round trip, Google OAuth if enabled for Beta, the selected
   server-side AI provider, and Antigravity execution/interruption behavior.
5. The website-served installer is the tested installer by hash, and Windows
   x64 Beta messaging does not claim unsupported platforms.
6. Gitleaks, renderer secret scanning, traceability and specification checks
   pass for the release tree.

### Beta stabilized

Beta Stabilized means at least three clean repeat runs across install, sign-in,
project takeover, execution, interruption, recovery, evidence export and
uninstall; every expected external outage has a truthful user-facing state;
diagnostic/support output is bounded and secret-safe; crash/recovery and local
data backup behavior are documented; and no P0/P1 Beta defect remains open.

### Production ready

Production readiness requires a selected deployment topology with permanent
domains and ingress, managed PostgreSQL with tested backup/restore, secret
rotation, least-privilege service identities, production auth and tenant
authorization, observability and incident response, reproducible release
artifacts, signed Windows packages, a real signed updater or an explicit
documented release-only update policy, security review, privacy/legal surfaces,
and a release candidate that passes the platform-specific clean-machine gates.

### Commercially ready

Commercial readiness requires founder-approved billing provider, pricing,
tax/refund policy, subscription lifecycle, entitlements, founder complimentary
access, selected-company grants, organization/team policy, support ownership,
onboarding, account deletion/export policy and operational reconciliation. A
billing status route alone is not commercial readiness.

### Mature multi-IDE product

Maturity requires a stable IDE-neutral adapter contract, at least one certified
production adapter, independently certified additional adapters, capability-
based detection and onboarding, per-adapter evidence and interruption tests,
platform-specific release gates, organization policy controls, durable audit
exports, recovery across machines where intentionally supported, and a clear
support/compatibility matrix. Antigravity remains the first integration, not
Relintor's product identity.

## 3. Execution Rules for Future AI Engineers

1. Active repository source, executed evidence and release artifacts outrank
   historical documentation or transfer notes.
2. Never mark implementation `VERIFIED` without criterion-bound evidence.
3. Preserve the completion-authority boundary: renderer text, executor claims,
   AI output, mock stores and screenshots cannot mint completion.
4. Never use `MockAdapter`, memory stores, fake provider responses or local test
   credentials as production proof.
5. Inspect the relevant source, tests, config, migrations and traceability
   record before editing.
6. Keep changes scoped; do not redesign unrelated architecture or silently
   expand a task.
7. Preserve `spec/locked`, `Cargo.lock`, `pnpm-lock.yaml`, security checks and
   fail-closed behavior unless an explicitly approved task requires a change.
8. Never expose secret values in source, logs, documentation, screenshots or
   reports. Environment-variable names may be recorded.
9. Run the smallest relevant regression tests first, then the required release
   gates; record exact commands, exit codes and raw failures.
10. Treat unavailable external services as blocked/not-run, never as pass.
11. Update this plan, the affected canonical evidence document and the handoff
    pointer after each completed task.
12. Add newly discovered work as a new stable task ID; preserve completed task
    records and history.
13. Do not mark a phase complete until every phase exit criterion passes.
14. Do not touch GitHub or publish a release unless the task explicitly grants
    that authority.

## 4. Master Dependency Order

```text
REL-TASK-001 updater regression
  -> REL-TASK-002 repaired startup evidence
  -> REL-TASK-003 pinned MSVC Rust gates
  -> REL-TASK-004 website dist rebuild/hash alignment
  -> REL-TASK-005..REL-TASK-010 real Windows Beta runtime/release gates
  -> REL-TASK-011..REL-TASK-019 Beta stabilization
  -> REL-TASK-020..REL-TASK-030 production foundation
  -> REL-TASK-031..REL-TASK-038 commercial production
  -> REL-TASK-039..REL-TASK-043 IDE adapter expansion
  -> REL-TASK-044..REL-TASK-047 platform expansion
  -> REL-TASK-048..REL-TASK-055 advanced and mature-product capabilities
```

The dependency is evidence-driven: later phases may be designed in parallel,
but no later release claim may bypass an earlier failed or unrun gate.

## 5. Detailed Engineering Tasks

### REL-TASK-001 — Updater configuration regression

PHASE: 0 — Close Current Windows x64 Beta  
PRIORITY: P0  
STATUS: NEXT  
BLOCKED BY: none  
BLOCKS: REL-TASK-002, REL-TASK-003, Beta release certification

OBJECTIVE: Prevent a null or absent production updater configuration from
crashing Tauri startup, while keeping update commands unavailable and fail
closed until genuine signed configuration exists.

CURRENT EVIDENCE: The old binary logged a `plugins.updater` null deserialization
panic. Active `apps/desktop/src-tauri/tauri.conf.json` contains an object with
empty `pubkey` and `endpoints`; `apps/desktop/src-tauri/src/lib.rs` still
requires non-empty runtime updater values and rejects unsafe endpoints.

IMPLEMENTATION AREA: `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/src/lib.rs`, `apps/desktop/src-tauri/Cargo.toml`, and
the existing native test module in `apps/desktop/src-tauri/src/lib.rs`.

IMPLEMENTATION REQUIREMENTS: Add a focused regression test for object-shaped
inert configuration, missing production values, unsafe endpoint rejection and
unavailable update commands. Keep Tauri signature verification and the existing
runtime configuration boundary intact.

PRESERVE: Fail-closed updater behavior, no fake public key, no fake endpoint,
startup of all non-updater features.

DO NOT: Register a fake production update service, return success when updates
are unavailable, or weaken signature verification.

TESTS REQUIRED: `cargo fmt --all -- --check`; targeted native updater tests;
`cargo clippy --workspace --all-targets --locked -- -D warnings`.

NEW TESTS REQUIRED: A test that parses the active configuration and proves
missing/empty production provisioning is an explicit unavailable result rather
than a panic or false success.

MANUAL VERIFICATION: Inspect the built configuration and launch the repaired
executable with updater provisioning absent.

ACCEPTANCE CRITERIA: The regression test passes; the configuration is never
null; no updater command reports a successful update without valid signed
configuration; existing desktop tests remain green.

EVIDENCE TO RECORD: Test command/output, config hash or reviewed diff, runtime
unavailable result, and source/lock identity.

DONE WHEN: The active updater architecture is regression-tested and fail-closed.

DOCUMENTS TO UPDATE: `docs/RELINTOR_REPOSITORY_VERIFICATION.md`, this plan,
`docs/AI_HANDOFF.md`, and the applicable milestone evidence.

### REL-TASK-002 — Repaired executable startup proof

PHASE: 0  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-001  
BLOCKS: REL-TASK-005, Beta release certification

OBJECTIVE: Replace empty repaired launch logs with direct, reproducible process
evidence for the current native executable.

CURRENT EVIDENCE: `relintor-desktop.exe` exists with SHA-256
`A21DEFF2360FEEBD182C93E9D7B1EA729DBCFBDA24E28136044A4A6C5BB24AB9`; repaired
stdout/stderr logs are empty, but no repaired exit code is recorded.

IMPLEMENTATION AREA: `apps/desktop/src-tauri/src/main.rs`,
`apps/desktop/src-tauri/src/lib.rs`, existing executable artifact and the
current Windows release harness; no product-code change is assumed.

IMPLEMENTATION REQUIREMENTS: Run the exact artifact in a controlled process
with bounded stdout/stderr capture, timeout, exit code, executable hash and
configuration identity. If it stays resident, use a defined startup-health
observation and controlled termination rather than treating timeout as success.

PRESERVE: User data, machine-independent runtime paths and no secret capture.

DO NOT: Infer success from an empty log, browser preview, installer existence or
the historical old binary.

TESTS REQUIRED: Desktop frontend gates and the native no-bundle build after any
source change; no source change is expected for this task.

NEW TESTS REQUIRED: Add none unless the harness exposes a reproducible product
defect; record a new task instead of broadening this evidence task.

MANUAL VERIFICATION: Launch the exact SHA-256-matched executable on the build
machine with production updater values absent and capture exit/runtime health.

ACCEPTANCE CRITERIA: No startup panic; explicit successful startup observation
and exit/termination evidence; no secret values in logs.

EVIDENCE TO RECORD: Artifact path, size, SHA-256, timestamp, command, exit code,
timeout policy, stdout/stderr and environment classification.

DONE WHEN: The repaired executable has direct, reviewable startup evidence.

DOCUMENTS TO UPDATE: Repository verification, this plan and AI handoff.

### REL-TASK-003 — Pinned x64 MSVC Rust quality gates

PHASE: 0  
PRIORITY: P0  
STATUS: BLOCKED_ENVIRONMENT  
BLOCKED BY: x64 MSVC Rust 1.96 toolchain and Visual Studio C++ Build Tools in the
current runner  
BLOCKS: REL-TASK-010 and Beta release certification

OBJECTIVE: Produce fresh Rust evidence for the release tree under the pinned
x64 MSVC environment, never GNU fallback.

CURRENT EVIDENCE: `rust-toolchain.toml` requests Rust 1.96.0. The documentation
audit runner had no installed Rust toolchains and attempted a GNU sync; no
current Rust result was produced.

IMPLEMENTATION AREA: `rust-toolchain.toml`, root `Cargo.toml`, workspace crates
under `crates/`, services under `services/`, and `apps/desktop/src-tauri`.

IMPLEMENTATION REQUIREMENTS: Use the repository's pinned toolchain and an x64
MSVC developer environment; preserve lockfiles. Run fmt, clippy, workspace
tests, relintor-core tests and native Tauri no-bundle build.

PRESERVE: Rust 1.96 pin, MSVC target selection, D:-first development storage,
cross-platform repository configuration.

DO NOT: Fall back to GNU, hardcode a Windows target into shared configuration,
or classify an unexecuted command as pass.

TESTS REQUIRED: `cargo fmt --all -- --check`; `cargo clippy --workspace
--all-targets --locked -- -D warnings`; `cargo test --workspace --locked`;
`cargo test -p relintor-core --locked`; `pnpm --dir apps/desktop tauri build
--no-bundle`.

NEW TESTS REQUIRED: None unless a genuine defect appears.

MANUAL VERIFICATION: Record `rustc -vV`, `where cl`, `where link`, toolchain
path, host triple, disk space and exact command exit codes.

ACCEPTANCE CRITERIA: Every listed gate passes under `x86_64-pc-windows-msvc`.

EVIDENCE TO RECORD: Full command ledger, host/toolchain/linker evidence, test
totals, artifact hashes and raw failures if any.

DONE WHEN: Fresh MSVC evidence is bound to the current release source and locks.

DOCUMENTS TO UPDATE: Repository verification, this plan and current milestone
evidence.

### REL-TASK-004 — Website distribution hash alignment

PHASE: 0  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-003 only if the installer must be rebuilt  
BLOCKS: REL-TASK-010, Beta release certification

OBJECTIVE: Ensure the website-served download is the verified current Windows
x64 installer.

CURRENT EVIDENCE: `apps/website/public/downloads/Relintor_0.1.0_x64-setup.exe`
matches SHA-256 `95CD...BC250`, while `apps/website/dist/downloads/...` still
matches the older SHA-256 `9644...7199`.

IMPLEMENTATION AREA: `apps/website/public/downloads/`, `apps/website/dist/`,
`apps/website/src/App.tsx`, `apps/website/package.json` and the website build
configuration.

IMPLEMENTATION REQUIREMENTS: Rebuild the website from the current source and
public asset, verify the generated download hash and verify that the actual
deployment serves that same hash.

PRESERVE: Windows x64-only truthful Beta CTA and existing visual identity.

DO NOT: Rebuild desktop unnecessarily, claim ARM64/macOS/Linux, or publish a
stale dist artifact.

TESTS REQUIRED: `pnpm --dir apps/website typecheck`; `lint`; `test`; `build`.

NEW TESTS REQUIRED: A website test that checks the download href and Beta
platform label if the existing test does not already cover both.

MANUAL VERIFICATION: Hash `dist/downloads/...` and the deployed response.

ACCEPTANCE CRITERIA: Source, dist and served installer have the same expected
SHA-256; download is a real file response.

EVIDENCE TO RECORD: Build output, dist timestamp, size/hash and deployed URL
response/hash.

DONE WHEN: The public download chain is hash-aligned and truthful.

DOCUMENTS TO UPDATE: Repository verification, this plan, AI handoff and website
release evidence.

### REL-TASK-005 — Disposable Windows x64 clean-machine journey

PHASE: 0  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-002, REL-TASK-003, REL-TASK-004  
BLOCKS: REL-TASK-010, Phase 1

OBJECTIVE: Prove a normal Windows user can install and use the Beta without the
development repository, Rust, Cargo, pnpm, WSL or developer environment.

CURRENT EVIDENCE: A local x64 NSIS installer exists; clean-machine evidence is
not certified.

IMPLEMENTATION AREA: `packaging/windows/`, `apps/desktop/src-tauri/tauri.conf.json`,
desktop runtime configuration and `apps/desktop/src-tauri/icons/`.

IMPLEMENTATION REQUIREMENTS: Use a disposable standard-user Windows x64 machine
with paths containing spaces/non-ASCII characters where practical. Install,
launch, use local flow, close and relaunch. Record unsupported dependency states
truthfully.

PRESERVE: Machine-independent capability discovery and least privilege.

DO NOT: Copy development paths, require D:, require Rust/Node/pnpm, or fake
Antigravity/provider/cloud availability.

TESTS REQUIRED: Installer artifact hash, native startup, desktop UI tests and
the full Windows regression ledger.

NEW TESTS REQUIRED: Clean-machine scripted smoke coverage for install and first
launch where the current harness cannot reproduce it.

MANUAL VERIFICATION: Install as a standard user; test first launch, close,
relaunch, logs/support path and uninstall prompt.

ACCEPTANCE CRITERIA: Install and launch succeed without developer tooling;
unsupported services are clearly reported; no secrets or fixed developer paths
appear.

EVIDENCE TO RECORD: Machine OS/build/architecture, account type, installer
hash, install path, screenshots/logs, exit codes and cleanup result.

DONE WHEN: A clean Windows x64 Beta user journey is reproducible.

DOCUMENTS TO UPDATE: Repository verification, user/admin manual, this plan and
Beta evidence.

### REL-TASK-006 — Live PostgreSQL migration and account round trip

PHASE: 0  
PRIORITY: P0  
STATUS: BLOCKED_ENVIRONMENT  
BLOCKED BY: disposable local PostgreSQL instance and safe test credentials  
BLOCKS: REL-TASK-010, Phase 2 and commercial entitlement tasks

OBJECTIVE: Verify cloud account/session/entitlement behavior against real
PostgreSQL rather than the deterministic memory store.

CURRENT EVIDENCE: Migrations 002 and 003 and a PostgreSQL store exist; live
integration remains externally pending.

IMPLEMENTATION AREA: `services/cloud-api/`, `db/migrations/002_cloud_account_foundation.sql`,
`db/migrations/003_milestone2_reconciliation.sql`, `db/migrations/009_p10_teams_billing_admin.sql`,
`db/migrations/010_google_external_identities.sql`.

IMPLEMENTATION REQUIREMENTS: Use a disposable local PostgreSQL database; run
migrations, verify migration identities, create account/org/device/session,
look up and rotate sessions, retrieve entitlement policy and issue the bound
signed entitlement.

PRESERVE: Tenant binding, hashed/rotated sessions, signed entitlements and
fail-closed database readiness.

DO NOT: Replace the live database with SQL text parsing or a mock store.

TESTS REQUIRED: `cargo test -p relintor-cloud-api
mandatory_live_postgres_migration_and_account_round_trip --locked -- --ignored
--nocapture`; cloud API workspace tests.

NEW TESTS REQUIRED: Add only coverage for a discovered migration/runtime gap;
the mandatory live test is the gate.

MANUAL VERIFICATION: Record PostgreSQL version, disposable database identity
without credentials, migration 002/003 results and cleanup.

ACCEPTANCE CRITERIA: The ignored live test genuinely connects to PostgreSQL and
passes the full round trip.

EVIDENCE TO RECORD: Version, test output, migration identities, test database
scope, exit code and no credential values.

DONE WHEN: Real PostgreSQL evidence is accepted by the milestone verifier.

DOCUMENTS TO UPDATE: Repository verification, technical anatomy, this plan and
P2/P10 evidence.

### REL-TASK-007 — Google OAuth Beta exchange

PHASE: 0  
PRIORITY: P1  
STATUS: IMPLEMENTED_UNVERIFIED  
BLOCKED BY: real Google OAuth test configuration and safe test account  
BLOCKS: live-auth Beta certification

OBJECTIVE: Verify the existing system-browser PKCE and Google OIDC exchange in
the supported deployment without exposing client secrets.

CURRENT EVIDENCE: `/v1/auth/google/exchange`, issuer/audience/expiry/subject,
verified-email and JWKS signature validation are source-backed; live Google
authorization is not proven.

IMPLEMENTATION AREA: `apps/desktop/src-tauri/src/lib.rs`,
`apps/desktop/src/cloudApi.ts`, `services/cloud-api/src/lib.rs`,
`db/migrations/010_google_external_identities.sql`.

IMPLEMENTATION REQUIREMENTS: Verify authorization, callback timeout/cancel,
verified identity mapping, session issuance, refresh rotation and sign-out in
the intended environment.

PRESERVE: PKCE, issuer/audience/signature checks, verified-email policy and
server-side secret boundaries.

DO NOT: Log tokens, use arbitrary bearer values, or replace Google with a mock
for a live certification claim.

TESTS REQUIRED: Existing desktop/cloud auth tests; traceability and secret scan.

NEW TESTS REQUIRED: Add a deterministic callback timeout/cancel regression test
if the existing suite lacks it.

MANUAL VERIFICATION: Use a dedicated test account and capture only redacted
status, request IDs and exit results.

ACCEPTANCE CRITERIA: Real exchange succeeds for an approved account and rejects
invalid issuer/audience/signature/email cases.

EVIDENCE TO RECORD: Provider environment classification, redacted flow result,
session rotation result, exit code and cleanup.

DONE WHEN: Live Google evidence is independently repeatable and secret-safe.

DOCUMENTS TO UPDATE: Repository verification, user/admin manual, this plan and
auth evidence.

### REL-TASK-008 — Live server-side AI provider smoke

PHASE: 0  
PRIORITY: P1  
STATUS: REQUIRES PRODUCT DECISION  
BLOCKED BY: approved provider/deployment choice and safe server-side test key  
BLOCKS: live AI Beta certification

OBJECTIVE: Verify one approved real provider through the AI gateway without
putting credentials in the desktop or renderer.

CURRENT EVIDENCE: DeepSeek and OpenAI adapter boundaries exist, but the live
provider is not proven and no provider should be selected silently.

IMPLEMENTATION AREA: `services/ai-gateway/src/lib.rs`,
`services/ai-gateway/src/main.rs`, `apps/desktop/src/cloudApi.ts`,
`packages/contracts/src/cloudApi.ts`.

IMPLEMENTATION REQUIREMENTS: After provider selection, run a bounded real call,
timeout/outage path, usage accounting and authorization check. Record model and
provider names, never credentials or prompt-sensitive content.

PRESERVE: Server-side keys, bounded request/response, deadline, usage limits,
fail-closed mock policy and evidence authority.

DO NOT: Call providers from the desktop, hardcode a key, or promote a mock call.

TESTS REQUIRED: AI gateway tests, workspace clippy/tests, secret scan.

NEW TESTS REQUIRED: Provider-specific contract tests after the founder chooses
the provider.

MANUAL VERIFICATION: Real test request, forced timeout/outage and usage ledger
observation with redacted content.

ACCEPTANCE CRITERIA: Approved provider returns through the gateway; timeout,
outage, unauthorized and budget cases are truthful and bounded.

EVIDENCE TO RECORD: Provider choice, endpoint class, model, request ID, status,
latency/deadline and usage result without secrets.

DONE WHEN: The selected provider has a repeatable live gateway receipt.

DOCUMENTS TO UPDATE: Repository verification, technical anatomy, this plan and
AI gateway evidence.

### REL-TASK-009 — Antigravity Beta runtime certification

PHASE: 0  
PRIORITY: P0  
STATUS: IMPLEMENTED_UNVERIFIED  
BLOCKED BY: compatible real Antigravity installation and signed/approved bridge
package or documented Beta bridge path  
BLOCKS: REL-TASK-010, Phase 4 production adapter

OBJECTIVE: Prove the first executor integration works through capability-based
discovery, bounded execution, interruption and evidence handoff.

CURRENT EVIDENCE: `crates/relintor-antigravity/` and
`integrations/antigravity/` contain discovery/protocol/process-safety and bridge
manifest foundations; production signed bridge and real interruption/reboot
evidence remain pending.

IMPLEMENTATION AREA: `crates/relintor-antigravity/`,
`integrations/antigravity/compatibility/`, `integrations/antigravity/hooks/`,
`integrations/antigravity/plugin/`, `apps/desktop/src-tauri/src/lib.rs`.

IMPLEMENTATION REQUIREMENTS: Detect supported installations without fixed paths,
validate capability/version/protocol/signature, execute a disposable fixture,
interrupt/recover and bind output to Relintor evidence.

PRESERVE: IDE-neutral boundary, scope restrictions, no GUI click automation,
completion authority and fail-closed unsupported states.

DO NOT: Treat a filename, comment, mock adapter or “done” transcript as proof.

TESTS REQUIRED: Existing Antigravity and execution crate tests; workspace gates;
secret scan.

NEW TESTS REQUIRED: Real installation discovery and interruption/reboot fixture
coverage where current tests are source-only.

MANUAL VERIFICATION: Use a disposable project and capture capability, process,
interruption, recovery and evidence IDs.

ACCEPTANCE CRITERIA: Supported runtime executes the bounded scenario; missing or
unsupported runtime is explicit and non-pass; no project escape occurs.

EVIDENCE TO RECORD: Runtime version/capabilities, bridge identity, process IDs,
exit states, recovery state and evidence hashes.

DONE WHEN: Antigravity is certified as the first real executor adapter for Beta.

DOCUMENTS TO UPDATE: Technical anatomy, user/admin manual, repository
verification, this plan and Antigravity evidence.

### REL-TASK-010 — Windows x64 Beta certification closure

PHASE: 0  
PRIORITY: P0  
STATUS: BLOCKED BY REL-TASK-001 through REL-TASK-009  
BLOCKED BY: all required local and real Beta gates above  
BLOCKS: Phase 1

OBJECTIVE: Issue one canonical Windows x64 Beta decision from fresh, bound
evidence.

CURRENT EVIDENCE: Current status is Windows x64 artifact present but Beta
deployment not closed.

IMPLEMENTATION AREA: `tooling/evidence/`, `tooling/acceptance/`,
`tooling/acceptance/implementation-traceability.json`, and the four canonical
documents in `docs/`.

IMPLEMENTATION REQUIREMENTS: Reconcile every required gate, preserve historical
failures as historical, and choose only a permitted verdict supported by the
evidence.

PRESERVE: Traceability IDs, evidence invalidation, P2/P3 carried debt and
cross-platform non-claims.

DO NOT: Certify from a stale milestone report, empty logs, hosted workflow files
or source-only implementation.

TESTS REQUIRED: Full Windows Beta command ledger, scoped Gitleaks, renderer
secret scan, website gates and all live runtime receipts.

NEW TESTS REQUIRED: Only task-specific gaps discovered by reconciliation.

MANUAL VERIFICATION: Independent review of hashes, machine identity, external
receipts and clean-machine artifacts.

ACCEPTANCE CRITERIA: All required Windows x64 Beta gates pass or are explicitly
classified as external blockers; no required local gate remains unresolved.

EVIDENCE TO RECORD: Canonical report, verdict, all command exit codes, hashes,
screenshots/receipts and remaining blockers.

DONE WHEN: The repository can truthfully say Beta complete or name the exact
blocker without ambiguity.

DOCUMENTS TO UPDATE: All four canonical docs, relevant milestone evidence and
this plan.

## 6. Phase 0 — Close Current Windows x64 Beta

REL-TASK-001 through REL-TASK-010 are the Phase 0 sequence. The first four
ordering is verified against the current repository record:

`updater regression -> explicit startup evidence -> pinned MSVC gates -> website
dist/hash alignment -> remaining real Beta runtime/release gates`.

The remaining gates are the clean-machine journey, live PostgreSQL, live Google
if enabled, approved live AI provider, real Antigravity runtime, installer and
uninstall behavior, signed/secret-safe artifact evidence and canonical Beta
certification. None may be silently collapsed into a source-only pass.

## 7. Phase 1 — Beta Stabilization

### REL-TASK-011 — Repeatable real-user workflow regression

PHASE: 1 — Beta Stabilization  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-010  
BLOCKS: REL-TASK-019

OBJECTIVE: Exercise install, sign-in, project creation/takeover, authority,
execution, activity/evidence, export and sign-out as one repeatable user story.

CURRENT EVIDENCE: The desktop renderer and native command boundary exist; only
local deterministic frontend evidence is current.

IMPLEMENTATION AREA: `apps/desktop/src/`, `apps/desktop/src-tauri/src/`,
`crates/relintor-takeover/`, `crates/relintor-execution/`,
`crates/relintor-evidence/`.

IMPLEMENTATION REQUIREMENTS: Add stable fixture/automation at the native
boundary, preserve authority sequencing and make every unavailable state visible.

PRESERVE: Local authority, evidence invalidation and no renderer completion.

DO NOT: Assert success from UI text alone.

TESTS REQUIRED: Desktop typecheck/lint/test, workspace tests and native build.

NEW TESTS REQUIRED: End-to-end workflow coverage using a disposable fixture.

MANUAL VERIFICATION: Three repeat runs on a clean Beta machine.

ACCEPTANCE CRITERIA: All runs produce consistent authority/evidence outcomes;
failure states are recoverable and truthful.

EVIDENCE TO RECORD: Test fixture identity, run IDs, evidence hashes and failure
recovery results.

DONE WHEN: The complete Beta user story is repeatable without developer tools.

DOCUMENTS TO UPDATE: User/admin manual, repository verification and this plan.

### REL-TASK-012 — Install, upgrade, uninstall and data policy

PHASE: 1  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-005, REL-TASK-010  
BLOCKS: REL-TASK-019, REL-TASK-030

OBJECTIVE: Make installer lifecycle and local data preservation predictable.

CURRENT EVIDENCE: Windows NSIS artifact exists; clean install/upgrade/uninstall
and export preservation are not certified. Technical anatomy identifies A-12 as
an integration gap.

IMPLEMENTATION AREA: `packaging/windows/`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/src/`, `crates/relintor-distribution/`.

IMPLEMENTATION REQUIREMENTS: Define upgrade compatibility, uninstall behavior,
evidence export/preservation, rollback on failed install and user messaging.

PRESERVE: User-owned local evidence and fail-closed integrity checks.

DO NOT: Delete evidence silently or claim rollback without testing it.

TESTS REQUIRED: Native build, distribution tests, installer hash and clean
machine lifecycle.

NEW TESTS REQUIRED: Install/upgrade/uninstall integration fixture.

MANUAL VERIFICATION: Fresh install, upgrade from prior Beta, failed upgrade,
uninstall and re-install with path/data inspection.

ACCEPTANCE CRITERIA: Documented data policy is observed and every lifecycle
outcome is explicit.

EVIDENCE TO RECORD: Installer versions/hashes, paths, before/after data inventory
and exit results.

DONE WHEN: Lifecycle behavior is safe and repeatable on clean Windows x64.

DOCUMENTS TO UPDATE: User/admin manual, technical anatomy, release evidence and
this plan.

### REL-TASK-013 — Interrupted execution and recovery hardening

PHASE: 1  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-009, REL-TASK-011  
BLOCKS: REL-TASK-019, REL-TASK-052

OBJECTIVE: Prove interruption, crash, reboot and resume never produce false
completion.

CURRENT EVIDENCE: `crates/relintor-execution/src/recovery.rs` and P7/P9 tests
exist; real runtime interruption/reboot evidence is not current.

IMPLEMENTATION AREA: `crates/relintor-execution/`,
`apps/desktop/src-tauri/src/`, `tooling/evidence/`.

IMPLEMENTATION REQUIREMENTS: Persist checkpoints, classify interrupted work,
invalidate stale evidence and require safe resume/revalidation.

PRESERVE: Completion authority, workspace fingerprints and fail-closed recovery.

DO NOT: Resume from an unbound transcript or convert timeout into success.

TESTS REQUIRED: Existing P7/P9 recovery tests, workspace clippy/tests and native
desktop gates.

NEW TESTS REQUIRED: Real process-kill/reboot fixture and repeated resume test.

MANUAL VERIFICATION: Interrupt a disposable run at each defined checkpoint.

ACCEPTANCE CRITERIA: Every interruption yields a safe state; only valid resumed
evidence can advance authority.

EVIDENCE TO RECORD: Checkpoint IDs, process/exit state, restart state and
authority decision.

DONE WHEN: Recovery is tested in runtime, not only in unit fixtures.

DOCUMENTS TO UPDATE: Technical anatomy, user manual, repository verification and
this plan.

### REL-TASK-014 — Secure storage and credential lifecycle

PHASE: 1  
PRIORITY: P1  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-005  
BLOCKS: REL-TASK-019, REL-TASK-022

OBJECTIVE: Verify tokens, local keys and sensitive state use the intended Windows
secure-storage boundary and survive only the documented lifecycle.

CURRENT EVIDENCE: Desktop auth and local state paths exist; clean-machine and
keychain evidence is not current.

IMPLEMENTATION AREA: `apps/desktop/src-tauri/src/`,
`apps/desktop/src/backend.ts`, `apps/desktop/src/cloudApi.ts`, and existing
desktop Cargo dependencies/configuration.

IMPLEMENTATION REQUIREMENTS: Inventory stored material, expiration, logout,
rotation, access errors and migration behavior; add capability-based diagnostics.

PRESERVE: No provider key in desktop, logs or renderer; least privilege.

DO NOT: Store bearer tokens in plaintext files or claim secure storage from a
configuration label alone.

TESTS REQUIRED: Desktop tests, secret scan, native tests and clean-machine run.

NEW TESTS REQUIRED: Storage failure, logout cleanup and refresh rotation tests.

MANUAL VERIFICATION: Inspect only metadata/path permissions, never values.

ACCEPTANCE CRITERIA: Stored credentials are bounded, revocable and not exposed
through diagnostics or logs.

EVIDENCE TO RECORD: Storage mechanism, permission observations and redacted
rotation/logout results.

DONE WHEN: Secure storage behavior is verified on supported Windows x64.

DOCUMENTS TO UPDATE: Technical anatomy, user manual, security evidence and plan.

### REL-TASK-015 — Diagnostics and support bundle safety

PHASE: 1  
PRIORITY: P1  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-011, REL-TASK-014  
BLOCKS: REL-TASK-019, REL-TASK-037

OBJECTIVE: Give Beta users and support a bounded diagnostic path without leaking
tokens, keys, raw project content or private signing material.

CURRENT EVIDENCE: Desktop diagnostics/export concepts and secret scanning exist;
clean support-bundle evidence is not certified.

IMPLEMENTATION AREA: `apps/desktop/src-tauri/src/`, `apps/desktop/src/`,
`crates/relintor-evidence/`, `tooling/acceptance/secret_scan.py`.

IMPLEMENTATION REQUIREMENTS: Define fields, redaction, retention, user consent,
size bounds and incident correlation IDs.

PRESERVE: Evidence integrity and server-side secret boundaries.

DO NOT: Include raw bearer tokens, provider keys, database URLs or unrestricted
project files.

TESTS REQUIRED: Secret scan, desktop tests, evidence tests and bounded-output
tests.

NEW TESTS REQUIRED: Redaction corpus and maximum-size diagnostic bundle tests.

MANUAL VERIFICATION: Generate and inspect a redacted bundle from a clean Beta
machine.

ACCEPTANCE CRITERIA: Support can diagnose defined failures without sensitive
values or unbounded content.

EVIDENCE TO RECORD: Bundle manifest, size/hash, redaction test output and review.

DONE WHEN: Safe diagnostics are usable by a non-developer.

DOCUMENTS TO UPDATE: User manual, technical anatomy, security evidence and plan.

### REL-TASK-016 — Runtime failure and offline UX

PHASE: 1  
PRIORITY: P1  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-011, REL-TASK-015  
BLOCKS: REL-TASK-019

OBJECTIVE: Make network, auth, provider, Antigravity, database and updater
failures visible, bounded and recoverable without false green states.

CURRENT EVIDENCE: Fail-closed behavior is represented in source and docs;
real-user failure UX is not fully certified.

IMPLEMENTATION AREA: `apps/desktop/src/`, `apps/desktop/src-tauri/src/`,
`apps/admin/src/`, `services/cloud-api/src/`, `services/ai-gateway/src/`.

IMPLEMENTATION REQUIREMENTS: Map errors to stable user states, retry policy,
cancel behavior, offline local behavior and support identifiers.

PRESERVE: Authority boundaries and explicit blocked/unverified semantics.

DO NOT: Show a generic success toast for a failed cloud/provider/executor call.

TESTS REQUIRED: Frontend suites, service tests, workspace tests and native build.

NEW TESTS REQUIRED: Network loss, timeout, malformed response and retry-loop
coverage.

MANUAL VERIFICATION: Disable network or service dependency during each journey.

ACCEPTANCE CRITERIA: Users know what happened, what is safe to retry and what
requires support; no false completion is possible.

EVIDENCE TO RECORD: Scenario matrix, screenshots, request IDs and state results.

DONE WHEN: Defined runtime failures have reviewed UX and regression coverage.

DOCUMENTS TO UPDATE: User manual, technical anatomy, repository verification and
plan.

### REL-TASK-017 — Beta UX and accessibility closure

PHASE: 1  
PRIORITY: P1  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-011, REL-TASK-016  
BLOCKS: REL-TASK-019

OBJECTIVE: Resolve documented UX defects in the real Beta workflows without
changing authority architecture.

CURRENT EVIDENCE: Desktop/admin/website React surfaces and tests exist; no
complete non-expert accessibility/usability certification is current.

IMPLEMENTATION AREA: `apps/desktop/src/`, `apps/admin/src/`, `apps/website/src/`,
`packages/ui/` if present for the affected component.

IMPLEMENTATION REQUIREMENTS: Review keyboard flow, focus, loading/error/empty
states, labels, contrast, responsive layout and truthful platform messaging.

PRESERVE: Official Relintor branding and existing product identity.

DO NOT: Hide blocked states or alter copy to imply unsupported capability.

TESTS REQUIRED: Each affected app's typecheck/lint/test/build.

NEW TESTS REQUIRED: Accessibility and user-journey regression tests for each
resolved defect.

MANUAL VERIFICATION: Keyboard and screen-reader-oriented review on clean Beta.

ACCEPTANCE CRITERIA: Critical journeys are understandable and operable without
developer knowledge; no unsupported platform claims appear.

EVIDENCE TO RECORD: Defect IDs, before/after screenshots, test output and review.

DONE WHEN: Beta UX acceptance list has no unresolved P0/P1 defects.

DOCUMENTS TO UPDATE: User manual, website/admin notes, this plan.

### REL-TASK-018 — Stable Beta service and release environment

PHASE: 1  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-006, REL-TASK-007, REL-TASK-008, REL-TASK-009  
BLOCKS: REL-TASK-019, Phase 2

OBJECTIVE: Establish a repeatable Beta environment with explicit configuration,
secrets, migrations, service health and rollback procedure.

CURRENT EVIDENCE: Cloud and AI services have manifests and environment names;
production infrastructure is not represented as a certified deployment.

IMPLEMENTATION AREA: `services/cloud-api/`, `services/ai-gateway/`, `db/`,
`infra/`, `packages/config/`, and existing deployment documentation.

IMPLEMENTATION REQUIREMENTS: Version configuration templates, health/readiness,
migration order, safe secret injection, rollback and test data cleanup.

PRESERVE: No secrets in repository or desktop; `/readyz` must reflect actual
dependencies.

DO NOT: Call a memory store or mock provider a Beta production environment.

TESTS REQUIRED: Service tests, live PostgreSQL gate, secret scan and deployment
smoke tests.

NEW TESTS REQUIRED: Configuration completeness/readiness and rollback fixture.

MANUAL VERIFICATION: Deploy to a disposable environment and exercise health,
auth, AI and migration rollback/forward behavior.

ACCEPTANCE CRITERIA: A named disposable/staging environment is reproducible,
observable and safely disposable.

EVIDENCE TO RECORD: Config version, migration log, health responses, deployment
IDs and rollback result, without secret values.

DONE WHEN: Beta services can be recreated by another engineer.

DOCUMENTS TO UPDATE: Technical anatomy, operations docs, repository verification
and this plan.

### REL-TASK-019 — Stabilized Beta gate

PHASE: 1  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-011 through REL-TASK-018  
BLOCKS: Phase 2

OBJECTIVE: Freeze a stabilized Beta baseline and prove repeatability.

CURRENT EVIDENCE: Current status is not Beta-closed; stabilization evidence is
not a single accepted baseline.

IMPLEMENTATION AREA: `tooling/evidence/`, `tooling/acceptance/`, all affected
app/service/crate test areas and canonical `docs/`.

IMPLEMENTATION REQUIREMENTS: Run the complete matrix three times, reconcile
known defects, fingerprint source/locks/environment and publish a single
canonical stabilized report.

PRESERVE: Historical evidence labeling and cross-platform non-claims.

DO NOT: Reuse stale artifacts after source/config changes.

TESTS REQUIRED: Full Windows Beta matrix, Gitleaks, secret scan, frontend gates,
Rust gates, live external gates and clean-machine journey.

NEW TESTS REQUIRED: Any regression exposed by repeated runs, each with a new
task ID if it extends scope.

MANUAL VERIFICATION: Independent review of the three run records.

ACCEPTANCE CRITERIA: No P0/P1 stabilization defect remains; all required Beta
gates are fresh and bound.

EVIDENCE TO RECORD: Run matrix, hashes, defects, blockers and final verdict.

DONE WHEN: Phase 1 exit criteria are accepted.

DOCUMENTS TO UPDATE: All canonical docs, milestone evidence and plan pointer.

## 8. Phase 2 — Production Foundation

### REL-TASK-020 — Permanent domains and ingress

PHASE: 2 — Production Foundation  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-019, founder domain/deployment decision  
BLOCKS: REL-TASK-021, REL-TASK-030

OBJECTIVE: Replace temporary/local endpoints with a documented production
topology and capability-validated HTTPS ingress.

CURRENT EVIDENCE: Environment-variable endpoint names and service routes exist;
permanent production domains are not verified in the repository.

IMPLEMENTATION AREA: `infra/`, `services/cloud-api/`, `services/ai-gateway/`,
`apps/desktop/src-tauri/src/`, `apps/desktop/src/cloudApi.ts`.

IMPLEMENTATION REQUIREMENTS: Select domains, TLS, DNS, rate limits, health,
timeouts, origin policy and environment separation.

PRESERVE: Runtime machine independence and HTTPS production requirements.

DO NOT: Hardcode this development machine's paths or endpoints.

TESTS REQUIRED: Service tests, desktop tests, readiness checks and security scan.

NEW TESTS REQUIRED: HTTPS/host validation and wrong-environment rejection.

MANUAL VERIFICATION: Resolve and exercise each production endpoint from a clean
client with redacted request IDs.

ACCEPTANCE CRITERIA: Production endpoints are stable, HTTPS-only and separable
from staging/test environments.

EVIDENCE TO RECORD: Domain ownership, certificate/expiry metadata, health and
routing results; never private keys.

DONE WHEN: Production ingress is repeatable and documented.

DOCUMENTS TO UPDATE: Technical anatomy, operations docs, plan and handoff.

### REL-TASK-021 — Production configuration and secret operations

PHASE: 2  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-020  
BLOCKS: REL-TASK-022 through REL-TASK-030

OBJECTIVE: Make production configuration complete, auditable, rotatable and
separate from local/development modes.

CURRENT EVIDENCE: Environment names are source-visible and secret scanning
passes; no production secret-management run is certified.

IMPLEMENTATION AREA: `packages/config/`, `services/cloud-api/`,
`services/ai-gateway/`, `infra/`, `tooling/acceptance/secret_scan.py`.

IMPLEMENTATION REQUIREMENTS: Define required/optional variables, startup
validation, secret injection, rotation, redaction and environment identity.

PRESERVE: Server-side provider/database/signing secrets and fail-closed startup.

DO NOT: Put secrets in lockfiles, installers, renderer bundles or docs.

TESTS REQUIRED: Secret scan, config tests, service tests and deployment smoke.

NEW TESTS REQUIRED: Missing/expired/rotated-secret configuration tests.

MANUAL VERIFICATION: Rotate a disposable secret and verify old material fails.

ACCEPTANCE CRITERIA: Production starts only with complete safe configuration;
rotation has an observed, reversible procedure.

EVIDENCE TO RECORD: Variable inventory by name/class, rotation timestamps and
redacted startup/health results.

DONE WHEN: Another operator can deploy without guessing configuration semantics.

DOCUMENTS TO UPDATE: Technical anatomy, operations docs, security evidence and
plan.

### REL-TASK-022 — Production authentication hardening

PHASE: 2  
PRIORITY: P0  
STATUS: IMPLEMENTED_UNVERIFIED  
BLOCKED BY: REL-TASK-020, REL-TASK-021, live auth environment  
BLOCKS: REL-TASK-023, REL-TASK-030

OBJECTIVE: Harden and live-verify Google/session/device authentication for
production tenants.

CURRENT EVIDENCE: Google OIDC checks, device/session routes and refresh rotation
are source-backed; live production auth is not certified.

IMPLEMENTATION AREA: `services/cloud-api/src/lib.rs`, `services/cloud-api/src/p10.rs`,
`apps/desktop/src-tauri/src/lib.rs`, `apps/desktop/src/cloudApi.ts`,
`db/migrations/010_google_external_identities.sql`.

IMPLEMENTATION REQUIREMENTS: Enforce issuer/audience/JWKS/email policy, device
binding, refresh replay rejection, logout/revocation, rate limits and safe
error messages.

PRESERVE: PKCE and server-side identity authority.

DO NOT: Accept arbitrary bearer strings, skip signature checks or use dev sign-in
outside its explicitly permitted environment.

TESTS REQUIRED: Auth tests, live Google test when configured, clippy/workspace
tests, secret scan.

NEW TESTS REQUIRED: Replay, clock skew, JWKS rotation, logout and tenant-bound
device cases.

MANUAL VERIFICATION: Redacted production-like login/refresh/logout journey.

ACCEPTANCE CRITERIA: Invalid tokens and replay fail; valid users receive only
their tenant-bound access.

EVIDENCE TO RECORD: Redacted auth outcomes, policy version, JWKS identity and
replay rejection output.

DONE WHEN: Production auth is live-tested and security-reviewed.

DOCUMENTS TO UPDATE: Technical anatomy, user/admin manual, security report and
plan.

### REL-TASK-023 — Tenant, role and admin authorization hardening

PHASE: 2  
PRIORITY: P0  
STATUS: IMPLEMENTED_UNVERIFIED  
BLOCKED BY: REL-TASK-022, REL-TASK-006  
BLOCKS: REL-TASK-030, commercial tasks

OBJECTIVE: Prove every organization, membership, entitlement, grant and admin
operation is tenant- and role-scoped.

CURRENT EVIDENCE: Cloud routes, memberships, admin/MFA, grants and audit tables
exist; live PostgreSQL authorization evidence is pending.

IMPLEMENTATION AREA: `services/cloud-api/src/lib.rs`, `services/cloud-api/src/p10.rs`,
`db/migrations/002_cloud_account_foundation.sql`,
`db/migrations/003_milestone2_reconciliation.sql`,
`db/migrations/009_p10_teams_billing_admin.sql`, `apps/admin/src/`.

IMPLEMENTATION REQUIREMENTS: Build an endpoint-by-endpoint authorization
matrix, enforce organization scope, admin MFA and audit events, and test
cross-tenant denial.

PRESERVE: Server authorization; admin UI never grants authority.

DO NOT: Trust client organization IDs, role labels or UI visibility.

TESTS REQUIRED: Cloud API tests, live PostgreSQL gate and frontend admin tests.

NEW TESTS REQUIRED: Cross-tenant, stale-role, missing-MFA and grant abuse tests.

MANUAL VERIFICATION: Two-tenant disposable environment with operator/admin/user
roles.

ACCEPTANCE CRITERIA: Unauthorized cross-tenant and un-MFA'd admin actions fail;
allowed actions are audited.

EVIDENCE TO RECORD: Authorization matrix, denial results, audit IDs and database
version.

DONE WHEN: Production authorization is independently reviewed.

DOCUMENTS TO UPDATE: Technical anatomy, admin manual, security evidence and plan.

### REL-TASK-024 — Backup, restore and migration operations

PHASE: 2  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-006, REL-TASK-020, REL-TASK-021  
BLOCKS: REL-TASK-030, REL-TASK-052

OBJECTIVE: Protect cloud identity, entitlement, grant, audit and usage data with
tested recovery operations.

CURRENT EVIDENCE: Ten migrations and PostgreSQL store exist; backup/restore
operations are not represented as certified infrastructure.

IMPLEMENTATION AREA: `db/migrations/`, `db/schema/`, `db/seeds/`, `services/cloud-api/`,
`infra/`.

IMPLEMENTATION REQUIREMENTS: Define backup frequency/retention, encryption,
restore order, migration compatibility, point-in-time recovery and verification.

PRESERVE: Tenant isolation, audit integrity and migration identity.

DO NOT: Claim a backup is restorable because a file exists.

TESTS REQUIRED: Migration tests, live PostgreSQL integration and restore smoke.

NEW TESTS REQUIRED: Automated restore integrity and rollback/forward migration
fixtures.

MANUAL VERIFICATION: Restore a disposable copy and verify account, entitlements,
usage and audit records.

ACCEPTANCE CRITERIA: RPO/RTO targets are measured and restore passes without
cross-tenant corruption.

EVIDENCE TO RECORD: Backup ID, encryption/retention class, restore timestamps,
checksums and row-level integrity summary without private data.

DONE WHEN: Operations can recover production data within approved targets.

DOCUMENTS TO UPDATE: Operations docs, technical anatomy, security report and plan.

### REL-TASK-025 — Windows code signing and trust chain

PHASE: 2  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: signing authority/certificate decision  
BLOCKS: REL-TASK-026, REL-TASK-030

OBJECTIVE: Produce trusted, tamper-evident Windows x64 release artifacts.

CURRENT EVIDENCE: Local x64 installer exists; production signing is not
certified; `packaging/signing/` is a placeholder area.

IMPLEMENTATION AREA: `packaging/signing/`, `packaging/windows/`,
`apps/desktop/src-tauri/tauri.conf.json`, release tooling and CI workflow.

IMPLEMENTATION REQUIREMENTS: Select certificate/storage, signing identity,
timestamping, verification, renewal and compromise response.

PRESERVE: Reproducible source/build identity and no private key in repository.

DO NOT: Treat an unsigned local artifact as a production release.

TESTS REQUIRED: Native build, installer hash, signature verification and secret
scan.

NEW TESTS REQUIRED: CI/release signature presence and wrong-certificate failure.

MANUAL VERIFICATION: Verify Authenticode chain on a clean machine.

ACCEPTANCE CRITERIA: Release artifacts verify to the approved publisher identity
and timestamp; unsigned artifacts cannot enter the release channel.

EVIDENCE TO RECORD: Public certificate fingerprint, signature verification,
artifact hashes and release job identity, never private key material.

DONE WHEN: Signed Windows release process is repeatable.

DOCUMENTS TO UPDATE: Release docs, repository verification, technical anatomy and
plan.

### REL-TASK-026 — Signed updater production path

PHASE: 2  
PRIORITY: P0  
STATUS: IMPLEMENTED_UNVERIFIED  
BLOCKED BY: REL-TASK-025, permanent HTTPS endpoint and approved updater key  
BLOCKS: REL-TASK-030

OBJECTIVE: Turn the currently inert updater into a genuinely configured,
cryptographically verified and rollback-safe production capability.

CURRENT EVIDENCE: `tauri-plugin-updater` is registered; active config is inert;
runtime builder requires non-empty key/endpoint; technical anatomy identifies
A-08 as an integration gap.

IMPLEMENTATION AREA: `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/src/lib.rs`, `apps/desktop/src-tauri/Cargo.toml`,
`packaging/signing/`, `packaging/windows/`.

IMPLEMENTATION REQUIREMENTS: Provision real signed metadata, HTTPS endpoint,
key rotation, version/channel policy, rollback/failure UX and staged rollout.

PRESERVE: Signature verification, unavailable state when unconfigured and
fail-closed update apply.

DO NOT: Add placeholder production keys/endpoints or bypass Tauri verification.

TESTS REQUIRED: Updater regression tests, signed/unsigned/tampered/replayed
artifact tests, native build and clean-machine update.

NEW TESTS REQUIRED: Key rotation, rollback and interrupted update tests.

MANUAL VERIFICATION: Install old signed build, update, interrupt, reject a
tampered build and roll back according to policy.

ACCEPTANCE CRITERIA: Only approved signed artifacts install; unavailable,
malformed, stale and failed updates are explicit non-pass states.

EVIDENCE TO RECORD: Key ID/public fingerprint, endpoint/version metadata, hashes,
signature results, rollout and rollback receipts.

DONE WHEN: Automatic signed updates are production-certified.

DOCUMENTS TO UPDATE: Technical anatomy, user manual, release evidence and plan.

### REL-TASK-027 — Reproducible release and artifact provenance

PHASE: 2  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-003, REL-TASK-025  
BLOCKS: REL-TASK-030, all platform releases

OBJECTIVE: Make a release reproducible and traceable from source/locks to
installer, website asset and signed metadata.

CURRENT EVIDENCE: Root lockfiles, pinned Rust toolchain, CI quality workflow and
local hashes exist; a complete provenance chain is not certified.

IMPLEMENTATION AREA: `Cargo.toml`, `Cargo.lock`, `pnpm-lock.yaml`,
`rust-toolchain.toml`, `.github/workflows/quality.yml`, `packaging/`, `tooling/`.

IMPLEMENTATION REQUIREMENTS: Record source revision, lock hashes, toolchain,
build inputs, artifact hashes, SBOM/provenance and promotion rules.

PRESERVE: Frozen lockfile policy and no developer-machine paths.

DO NOT: Rebuild from dirty/unrecorded source or publish an unbound artifact.

TESTS REQUIRED: Full CI quality matrix, native build, installer hash and secret
scan.

NEW TESTS REQUIRED: Rebuild comparison and provenance completeness gate.

MANUAL VERIFICATION: Rebuild in a clean environment and compare approved fields.

ACCEPTANCE CRITERIA: A reviewer can reproduce or cryptographically identify the
release inputs and outputs.

EVIDENCE TO RECORD: Revision/lock/toolchain IDs, hashes, provenance and signing
records.

DONE WHEN: Release provenance is complete and reviewable.

DOCUMENTS TO UPDATE: Release docs, repository verification and plan.

### REL-TASK-028 — Production observability and incident response

PHASE: 2  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-020, REL-TASK-021  
BLOCKS: REL-TASK-030, REL-TASK-037

OBJECTIVE: Observe service health, auth, usage, provider, migration and release
failures without collecting secrets or raw project content.

CURRENT EVIDENCE: Request IDs, bounded I/O, health/readiness and audit concepts
exist; production monitoring and response drills are not certified.

IMPLEMENTATION AREA: `services/cloud-api/`, `services/ai-gateway/`,
`apps/desktop/src-tauri/src/`, `tooling/evidence/`, `infra/`.

IMPLEMENTATION REQUIREMENTS: Define metrics, logs, traces, alerts, retention,
redaction, SLOs, incident ownership and runbooks.

PRESERVE: Secret-safe bounded diagnostics and tenant isolation.

DO NOT: Log bearer tokens, provider keys, database credentials or raw source.

TESTS REQUIRED: Service tests, secret scan, readiness checks and failure drills.

NEW TESTS REQUIRED: Alert/health regression and redaction tests.

MANUAL VERIFICATION: Simulate provider outage, database unavailability, auth
failure and elevated latency; confirm alerts and user states.

ACCEPTANCE CRITERIA: Operators detect and classify defined incidents within the
approved SLO and can correlate safe request IDs.

EVIDENCE TO RECORD: Dashboard/alert revision, drill timestamps and redacted logs.

DONE WHEN: Incident response is exercised, not merely documented.

DOCUMENTS TO UPDATE: Operations docs, technical anatomy, security report and plan.

### REL-TASK-029 — Security, privacy and threat-model review

PHASE: 2  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-021 through REL-TASK-028  
BLOCKS: REL-TASK-030, commercial launch

OBJECTIVE: Independently review the production attack surface and privacy data
flows before release.

CURRENT EVIDENCE: P1–P12 source audits and secret scan exist; production review,
privacy/legal decisions and live deployment testing remain incomplete.

IMPLEMENTATION AREA: All active source/configuration; especially
`apps/desktop/src-tauri/`, `services/cloud-api/`, `services/ai-gateway/`,
`integrations/antigravity/`, `db/` and `tooling/acceptance/`.

IMPLEMENTATION REQUIREMENTS: Review auth, tenant isolation, updater/signing,
execution scope, evidence integrity, provider data, retention and incident
response; classify findings with owners.

PRESERVE: Independent verification and fail-closed authority.

DO NOT: Close findings by weakening tests or changing labels only.

TESTS REQUIRED: All security/acceptance tests, Gitleaks and targeted adversarial
tests.

NEW TESTS REQUIRED: One regression for every accepted high-risk finding.

MANUAL VERIFICATION: Threat-model review and authorized penetration/security
assessment.

ACCEPTANCE CRITERIA: No unmitigated release-blocking finding; privacy data flow
and retention decisions are approved.

EVIDENCE TO RECORD: Review scope, findings, risk acceptance, fixes and retests.

DONE WHEN: Security and privacy sign-off is explicit.

DOCUMENTS TO UPDATE: Security/release docs, technical anatomy, plan and handoff.

### REL-TASK-030 — Production foundation release candidate

PHASE: 2  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-020 through REL-TASK-029  
BLOCKS: Phase 3

OBJECTIVE: Certify the first production foundation release candidate before
commercial exposure.

CURRENT EVIDENCE: Local x64 build and CI workflow exist; production foundation
is not certified.

IMPLEMENTATION AREA: `tooling/evidence/`, `tooling/acceptance/`, `packaging/`,
`.github/workflows/`, all production service/app areas.

IMPLEMENTATION REQUIREMENTS: Run source/lock/build/security/provenance,
infrastructure, backup/restore, signed release, updater and clean-machine gates.

PRESERVE: All historical evidence labels and platform-specific scope.

DO NOT: Call a local-only x64 artifact a production release.

TESTS REQUIRED: Full Windows matrix, service/live gates, signing/updater,
backup/restore, Gitleaks and independent release review.

NEW TESTS REQUIRED: Only gaps found by the release candidate review.

MANUAL VERIFICATION: Operator release rehearsal with rollback.

ACCEPTANCE CRITERIA: Production foundation exit criteria pass with no open P0/P1
security or reliability blocker.

EVIDENCE TO RECORD: Candidate manifest, all gate outputs, artifact/provenance
hashes and go/no-go decision.

DONE WHEN: Phase 2 is certified and commercial work can safely use it.

DOCUMENTS TO UPDATE: All canonical docs, release evidence and plan pointer.

## 9. Phase 3 — Commercial Production Product

### REL-TASK-031 — Billing provider and commercial policy decision

PHASE: 3 — Commercial Production Product  
PRIORITY: P0  
STATUS: REQUIRES PRODUCT DECISION  
BLOCKED BY: founder decision  
BLOCKS: REL-TASK-032 through REL-TASK-038

OBJECTIVE: Select the commercial model and billing provider before implementation
is treated as production scope.

CURRENT EVIDENCE: `/v1/billing/status`, subscriptions, plans and entitlement
foundations exist; the provider, pricing and launch policy are not established
by current repository evidence.

IMPLEMENTATION AREA: `services/cloud-api/src/p10.rs`, `services/cloud-api/src/lib.rs`,
`db/migrations/009_p10_teams_billing_admin.sql`, `apps/admin/src/`.

IMPLEMENTATION REQUIREMENTS: Decide provider, products/prices, trials, taxes,
refunds, cancellation, grace, webhooks, reconciliation and support ownership.

PRESERVE: Server-side entitlement authority and auditability.

DO NOT: Choose a provider, price or legal policy autonomously.

TESTS REQUIRED: Existing billing/entitlement tests as a baseline after decision.

NEW TESTS REQUIRED: Provider webhook/signature/idempotency contract tests.

MANUAL VERIFICATION: Founder-approved policy review and provider sandbox.

ACCEPTANCE CRITERIA: Signed decision records every commercial input needed by
engineering.

EVIDENCE TO RECORD: Decision ID, provider/product IDs in a secret-safe form,
policy and unresolved questions.

DONE WHEN: REL-TASK-032 has an approved provider and policy.

DOCUMENTS TO UPDATE: Plan, user/admin manual, technical anatomy and decision log.

### REL-TASK-032 — Subscription and entitlement production lifecycle

PHASE: 3  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-031, REL-TASK-030  
BLOCKS: REL-TASK-035, REL-TASK-038

OBJECTIVE: Implement and verify subscription-to-entitlement lifecycle for the
approved commercial policy.

CURRENT EVIDENCE: Plans, entitlements, grants and billing status foundations
exist in migrations and cloud routes; live commercial lifecycle is not proven.

IMPLEMENTATION AREA: `services/cloud-api/src/p10.rs`, `services/cloud-api/src/lib.rs`,
`db/migrations/009_p10_teams_billing_admin.sql`, `apps/admin/src/`,
`packages/contracts/src/cloudApi.ts`.

IMPLEMENTATION REQUIREMENTS: Model checkout/activation, renewal, cancellation,
payment failure, grace, expiry, refund and reconciliation idempotently.

PRESERVE: Organization binding, signed entitlement policy, usage limits and audit.

DO NOT: Let client billing state grant access or use an unverified webhook.

TESTS REQUIRED: Workspace tests, live PostgreSQL and provider sandbox contract.

NEW TESTS REQUIRED: Idempotent webhook, late event, refund and clock cases.

MANUAL VERIFICATION: Sandbox purchase-to-expiry and reconciliation journey.

ACCEPTANCE CRITERIA: Access follows approved billing state with safe retries and
complete audit history.

EVIDENCE TO RECORD: Redacted provider events, entitlement revisions and audit IDs.

DONE WHEN: Commercial lifecycle passes policy and security review.

DOCUMENTS TO UPDATE: User/admin manual, technical anatomy, plan and release gate.

### REL-TASK-033 — Founder complimentary access

PHASE: 3  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-031, REL-TASK-032  
BLOCKS: REL-TASK-038

OBJECTIVE: Provide founder-approved complimentary access with explicit scope,
expiry, revocation and audit.

CURRENT EVIDENCE: `complimentary_grants` and admin grant routes exist in the P10
foundation; production policy and live evidence are not certified.

IMPLEMENTATION AREA: `services/cloud-api/src/p10.rs`,
`db/migrations/003_milestone2_reconciliation.sql`,
`db/migrations/009_p10_teams_billing_admin.sql`, `apps/admin/src/`.

IMPLEMENTATION REQUIREMENTS: Define eligible identities/orgs, duration, limits,
approval, revocation, renewal and audit.

PRESERVE: Admin MFA, tenant scope and immutable audit event semantics.

DO NOT: Add hidden permanent bypasses or client-side founder flags.

TESTS REQUIRED: Admin/MFA/grant tests, live PostgreSQL and audit verification.

NEW TESTS REQUIRED: Expiry/revocation/duplicate grant and unauthorized grant tests.

MANUAL VERIFICATION: Founder/admin grant and revoke in a disposable organization.

ACCEPTANCE CRITERIA: Every grant is scoped, time-bounded or policy-bounded,
audited and revocable.

EVIDENCE TO RECORD: Grant policy/version, actor role, target scope, expiry and
audit ID without personal secrets.

DONE WHEN: Founder access is operationally safe and policy-approved.

DOCUMENTS TO UPDATE: Admin manual, user manual, plan and commercial evidence.

### REL-TASK-034 — Selected-company grants

PHASE: 3  
PRIORITY: P1  
STATUS: REQUIRES PRODUCT DECISION  
BLOCKED BY: founder eligibility/grant policy and REL-TASK-031  
BLOCKS: REL-TASK-038

OBJECTIVE: Support approved company-level complimentary or negotiated access
without bypassing organization authority.

CURRENT EVIDENCE: Company grant route and P10 admin foundation exist; company
selection, terms and limits are not established as product policy.

IMPLEMENTATION AREA: `services/cloud-api/src/p10.rs`, `apps/admin/src/`,
`db/migrations/009_p10_teams_billing_admin.sql`.

IMPLEMENTATION REQUIREMENTS: Founder must define selection, approval, quota,
duration, support and revocation; engineering then binds it to organizations.

PRESERVE: Tenant isolation, MFA, audit, usage accounting and signed entitlements.

DO NOT: Infer company eligibility from email domain or UI role alone.

TESTS REQUIRED: Grant/authorization/live PostgreSQL tests after policy approval.

NEW TESTS REQUIRED: Cross-organization and grant-boundary cases.

MANUAL VERIFICATION: Approved-company onboarding and revocation.

ACCEPTANCE CRITERIA: Only approved organizations receive the exact approved
terms, with complete audit and expiry behavior.

EVIDENCE TO RECORD: Decision, grant record, organization scope and audit trail.

DONE WHEN: Company grants are policy-approved and live-verified.

DOCUMENTS TO UPDATE: Admin manual, plan, commercial decision record.

### REL-TASK-035 — Organization/team product model

PHASE: 3  
PRIORITY: P0  
STATUS: IMPLEMENTED_UNVERIFIED  
BLOCKED BY: REL-TASK-023, REL-TASK-032, founder team-policy decision  
BLOCKS: REL-TASK-036, REL-TASK-038

OBJECTIVE: Make organization membership, roles, seats, policies, usage and
entitlements coherent for teams.

CURRENT EVIDENCE: Organizations, memberships, team policies, subscriptions,
usage and admin migrations/routes exist; live multi-user behavior is not proven.

IMPLEMENTATION AREA: `services/cloud-api/src/lib.rs`, `services/cloud-api/src/p10.rs`,
`db/migrations/002_cloud_account_foundation.sql`,
`db/migrations/003_milestone2_reconciliation.sql`,
`db/migrations/009_p10_teams_billing_admin.sql`, `apps/admin/src/`.

IMPLEMENTATION REQUIREMENTS: Define invite/accept/remove, role transitions,
seat/usage policy, ownership transfer, suspension and audit.

PRESERVE: Server-side membership and tenant binding.

DO NOT: Make the renderer the source of team authority.

TESTS REQUIRED: Cloud API, live PostgreSQL, admin frontend and auth tests.

NEW TESTS REQUIRED: Concurrent membership changes, last-owner, suspension and
seat-limit cases.

MANUAL VERIFICATION: Two-admin/multiple-member disposable organization journey.

ACCEPTANCE CRITERIA: Team policy is consistent across API, desktop and admin
surfaces and every mutation is audited.

EVIDENCE TO RECORD: Policy revision, membership events, usage and audit IDs.

DONE WHEN: The approved team model passes live authorization tests.

DOCUMENTS TO UPDATE: User/admin manuals, technical anatomy, plan.

### REL-TASK-036 — Production onboarding and account recovery

PHASE: 3  
PRIORITY: P1  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-022, REL-TASK-035, founder support/recovery policy  
BLOCKS: REL-TASK-038

OBJECTIVE: Make a new customer’s first login, organization setup, device
registration and recovery understandable and supportable.

CURRENT EVIDENCE: Desktop auth/device/account routes and admin surfaces exist;
production onboarding is not certified.

IMPLEMENTATION AREA: `apps/desktop/src/`, `apps/desktop/src-tauri/src/`,
`apps/admin/src/`, `services/cloud-api/src/`.

IMPLEMENTATION REQUIREMENTS: Define first-user/org creation, invite flow,
device replacement, lost-device recovery, logout and support escalation.

PRESERVE: PKCE, device/session binding and least privilege.

DO NOT: Ask users for provider keys or allow support to bypass audit.

TESTS REQUIRED: Desktop/admin/frontend and cloud auth tests; live auth where
available.

NEW TESTS REQUIRED: First-user, invite, recovery and device-revocation journeys.

MANUAL VERIFICATION: Non-expert user test with support observer.

ACCEPTANCE CRITERIA: A new customer can reach a usable organization without
manual database edits; recovery is safe and documented.

EVIDENCE TO RECORD: Redacted journey results, support path and audit events.

DONE WHEN: Onboarding is repeatable for the approved commercial model.

DOCUMENTS TO UPDATE: User/admin manual, support docs, plan.

### REL-TASK-037 — Support, privacy and legal product surfaces

PHASE: 3  
PRIORITY: P0  
STATUS: REQUIRES PRODUCT DECISION  
BLOCKED BY: retention, privacy, support and legal policy decisions  
BLOCKS: REL-TASK-038

OBJECTIVE: Publish truthful customer-facing support, privacy, retention and
account-data handling surfaces before paid release.

CURRENT EVIDENCE: Website/admin/desktop surfaces exist; final legal and privacy
policy content is not established by repository evidence.

IMPLEMENTATION AREA: `apps/website/src/`, `apps/admin/src/`, `apps/desktop/src/`,
`docs/`, and service retention/configuration areas after policy approval.

IMPLEMENTATION REQUIREMENTS: Founder/legal owner decides data categories,
retention, export/deletion, support SLA, incident contact and external provider
disclosures; implement only approved text and behavior.

PRESERVE: No false privacy/security promises and no raw project upload claim.

DO NOT: Let an AI author binding legal policy without approval.

TESTS REQUIRED: Website/admin/desktop UI gates, export/deletion tests and secret
scan.

NEW TESTS REQUIRED: Link/content presence and account data-policy regression.

MANUAL VERIFICATION: Review public pages and execute approved export/deletion
journey in a disposable account.

ACCEPTANCE CRITERIA: Published policy matches implemented retention/export/
support behavior and has owner approval.

EVIDENCE TO RECORD: Policy revision, approval, URL/build hash and behavior result.

DONE WHEN: Customers receive truthful support and data-policy information.

DOCUMENTS TO UPDATE: User/admin manual, website docs, plan and decision record.

### REL-TASK-038 — Controlled paid-release certification

PHASE: 3  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-031 through REL-TASK-037  
BLOCKS: Phase 4 and public release

OBJECTIVE: Certify a controlled commercial release to approved customers.

CURRENT EVIDENCE: Commercial data model foundations exist but provider/policy/
live operations are unresolved.

IMPLEMENTATION AREA: `tooling/evidence/`, `tooling/acceptance/`, `apps/admin/`,
`apps/desktop/`, `services/cloud-api/`, `db/`, `packaging/`.

IMPLEMENTATION REQUIREMENTS: Run paid, founder, company-grant, cancellation,
support, entitlement, usage, auth and incident scenarios with reconciliation.

PRESERVE: Auditability and no silent access bypass.

DO NOT: Release with unresolved product decisions or sandbox-only proof.

TESTS REQUIRED: Full release matrix plus provider sandbox/live evidence as
approved by policy.

NEW TESTS REQUIRED: Commercial regression cases discovered during rehearsal.

MANUAL VERIFICATION: Small controlled customer cohort and operator on-call.

ACCEPTANCE CRITERIA: Billing, access, support and rollback are operationally
consistent for the approved cohort.

EVIDENCE TO RECORD: Cohort/release IDs, entitlement and billing reconciliation,
support incidents and go/no-go approval.

DONE WHEN: Controlled paid release exit criteria pass.

DOCUMENTS TO UPDATE: All canonical docs, commercial/release evidence and plan.

## 10. Phase 4 — Multi-IDE Expansion

### REL-TASK-039 — Stable IDE-neutral executor contract

PHASE: 4 — Multi-IDE Expansion  
PRIORITY: P0  
STATUS: IMPLEMENTED_UNVERIFIED  
BLOCKED BY: REL-TASK-009, REL-TASK-038  
BLOCKS: REL-TASK-040 through REL-TASK-043

OBJECTIVE: Freeze the capability-based executor contract so Antigravity is an
adapter, not a product-wide assumption.

CURRENT EVIDENCE: `crates/relintor-antigravity/`, execution and takeover crates,
and integration manifests exist; a formally certified multi-IDE contract is not
current.

IMPLEMENTATION AREA: `crates/relintor-antigravity/`,
`crates/relintor-execution/`, `crates/relintor-contracts/`,
`integrations/antigravity/`.

IMPLEMENTATION REQUIREMENTS: Define discovery, capabilities, command lifecycle,
scope, cancellation, output bounds, evidence handoff, versioning and failure
states as adapter-neutral contracts.

PRESERVE: Completion authority, sandbox/scope checks and fail-closed unknown
adapter behavior.

DO NOT: Encode an IDE name into core authority or infer capability from a path.

TESTS REQUIRED: Existing execution/Antigravity/contracts tests and source audits.

NEW TESTS REQUIRED: Contract conformance fixture that every adapter must pass.

MANUAL VERIFICATION: Review protocol against a real first adapter.

ACCEPTANCE CRITERIA: An adapter can be replaced without changing authority or
evidence semantics.

EVIDENCE TO RECORD: Contract version, capability matrix and conformance output.

DONE WHEN: Contract is approved and versioned.

DOCUMENTS TO UPDATE: Technical anatomy, integration docs, plan.

### REL-TASK-040 — Production Antigravity adapter/package

PHASE: 4  
PRIORITY: P0  
STATUS: IMPLEMENTED_UNVERIFIED  
BLOCKED BY: REL-TASK-039, signing/package decision  
BLOCKS: REL-TASK-043

OBJECTIVE: Certify Antigravity as the first production adapter through the
stable contract.

CURRENT EVIDENCE: Compatibility registry, bridge manifest and source boundaries
exist; signed production bridge package and real runtime evidence are pending.

IMPLEMENTATION AREA: `crates/relintor-antigravity/`,
`integrations/antigravity/compatibility/registry.json`,
`integrations/antigravity/plugin/bridge-manifest.json`,
`integrations/antigravity/hooks/`, `packaging/signing/`.

IMPLEMENTATION REQUIREMENTS: Capability discovery, official runtime integration,
manifest signing/digest, installation/update/removal, process safety and
interruption/recovery certification.

PRESERVE: Antigravity is an adapter; no fixed path; no unsupported claims.

DO NOT: Ship an unsigned bridge or use GUI automation as the official protocol.

TESTS REQUIRED: Antigravity, execution, signing, clean-machine and workspace
gates.

NEW TESTS REQUIRED: Signed manifest tamper/expiry/compatibility and runtime
interrupt tests.

MANUAL VERIFICATION: Install, detect, execute, interrupt, update and remove the
adapter on supported Windows x64.

ACCEPTANCE CRITERIA: Only compatible signed adapter packages are accepted and
all authority/evidence rules remain intact.

EVIDENCE TO RECORD: Adapter/version/capabilities, signatures, process and
recovery receipts.

DONE WHEN: Antigravity production adapter is certified.

DOCUMENTS TO UPDATE: Integration docs, user manual, release evidence and plan.

### REL-TASK-041 — Founder decision on next IDE and compatibility policy

PHASE: 4  
PRIORITY: P1  
STATUS: REQUIRES PRODUCT DECISION  
BLOCKED BY: founder-supported IDE order and support policy  
BLOCKS: REL-TASK-042

OBJECTIVE: Choose the next IDE integration based on customer demand and support
capacity rather than AI preference.

CURRENT EVIDENCE: Antigravity is the first integration; no second IDE is
established by current repository evidence.

IMPLEMENTATION AREA: `integrations/antigravity/` as the reference contract and
`crates/relintor-antigravity/` as the existing adapter area.

IMPLEMENTATION REQUIREMENTS: Decide next IDE, supported versions/architectures,
official integration mechanism, licensing/support limits and acceptance cohort.

PRESERVE: IDE-agnostic core and capability-based detection.

DO NOT: Promise a named IDE without official compatibility and support evidence.

TESTS REQUIRED: Reference adapter contract suite.

NEW TESTS REQUIRED: Decision-specific compatibility corpus after selection.

MANUAL VERIFICATION: Founder/product review and vendor compatibility check.

ACCEPTANCE CRITERIA: A signed decision supplies scope for REL-TASK-042.

EVIDENCE TO RECORD: IDE, version, platform, integration mechanism, support and
reason for order.

DONE WHEN: Next adapter scope is approved.

DOCUMENTS TO UPDATE: Plan, technical anatomy and compatibility docs.

### REL-TASK-042 — Additional IDE adapter implementation

PHASE: 4  
PRIORITY: P1  
STATUS: FUTURE  
BLOCKED BY: REL-TASK-041, REL-TASK-039  
BLOCKS: REL-TASK-043

OBJECTIVE: Implement the founder-selected second IDE adapter without changing
Relintor's authority or evidence model.

CURRENT EVIDENCE: Only Antigravity adapter paths are present; the target IDE and
its integration files are not established.

IMPLEMENTATION AREA: New adapter work must remain under the verified adapter
boundary `crates/relintor-antigravity/` only if the contract is generalized
there, or an existing/new crate directory selected after source inspection; the
selected IDE's integration assets belong under `integrations/`.

IMPLEMENTATION REQUIREMENTS: Implement discovery, capabilities, lifecycle,
scope, cancellation, bounded output and evidence handoff using REL-TASK-039.

PRESERVE: Core authority and unsupported-state semantics.

DO NOT: Invent a path, protocol, vendor capability or support promise before the
founder decision and vendor evidence.

TESTS REQUIRED: Contract conformance, adapter unit/integration tests, source
audit and platform gates.

NEW TESTS REQUIRED: Selected IDE fixture, missing/unsupported/version mismatch,
escape and interruption tests.

MANUAL VERIFICATION: Real supported IDE install and bounded project execution.

ACCEPTANCE CRITERIA: The adapter passes the common contract and independent
security/evidence review.

EVIDENCE TO RECORD: Selected IDE/version, capability result, process/evidence
IDs, package hash and failure states.

DONE WHEN: Second adapter is production-certified for its stated scope.

DOCUMENTS TO UPDATE: Compatibility matrix, user manual, technical anatomy,
release evidence and plan.

### REL-TASK-043 — IDE integration certification gate

PHASE: 4  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-040, REL-TASK-042  
BLOCKS: Phase 5 and mature-product certification

OBJECTIVE: Ensure every supported adapter has independent integration,
security, onboarding and release evidence.

CURRENT EVIDENCE: P4–P5 source audits and Antigravity foundations exist; a
repeatable per-adapter production gate is not yet a certified release process.

IMPLEMENTATION AREA: `tooling/acceptance/`, `tooling/evidence/`,
`integrations/`, adapter crates and packaging directories.

IMPLEMENTATION REQUIREMENTS: Require contract version, signed package,
compatibility range, clean install, discovery, execute, interrupt, recovery,
uninstall and unsupported-state results.

PRESERVE: No adapter may mint completion.

DO NOT: Reuse Antigravity evidence for another adapter.

TESTS REQUIRED: Common contract suite plus adapter/platform tests and Gitleaks.

NEW TESTS REQUIRED: Certification harness parameterized by adapter identity.

MANUAL VERIFICATION: Independent reviewer repeats the matrix.

ACCEPTANCE CRITERIA: Each adapter has its own accepted evidence set and support
scope.

EVIDENCE TO RECORD: Adapter certificate inputs, hashes, versions, outcomes and
deferred platforms.

DONE WHEN: Multi-IDE support is evidence-backed rather than a marketing list.

DOCUMENTS TO UPDATE: Compatibility docs, repository verification, plan and handoff.

## 11. Phase 5 — Platform Expansion

Each platform task below is independent. A platform is not supported because
source configuration is portable or because CI has a workflow matrix entry.

### REL-TASK-044 — Windows ARM64 release

PHASE: 5 — Platform Expansion  
PRIORITY: P1  
STATUS: FUTURE  
BLOCKED BY: actual Antigravity/Relintor dependency support, ARM64 toolchain,
signing and test hardware  
BLOCKS: REL-TASK-047

OBJECTIVE: Certify Windows ARM64 only if the complete dependency and integration
stack supports it.

CURRENT EVIDENCE: ARM64 backup manifests exist, but local evidence is x64 MSVC
only and ARM64 is not certified.

IMPLEMENTATION AREA: `apps/desktop/src-tauri/`, `packaging/windows/`,
`apps/desktop/src-tauri/icons/`, `.github/workflows/`, `services/` only where
architecture-specific behavior is proven.

IMPLEMENTATION REQUIREMENTS: Build/package/sign, secure storage, updater,
Antigravity discovery/runtime, installer and clean-machine tests.

PRESERVE: Capability-based support claims and x64 behavior.

DO NOT: Claim ARM64 from a cross-compile or backup file.

TESTS REQUIRED: ARM64 fmt/clippy/tests/build, installer/signature/updater and
clean-machine matrix.

NEW TESTS REQUIRED: Architecture capability and unsupported-runtime tests.

MANUAL VERIFICATION: Real ARM64 Windows hardware/VM and supported Antigravity.

ACCEPTANCE CRITERIA: Every required ARM64 gate passes and support boundaries are
documented.

EVIDENCE TO RECORD: Hardware/OS, toolchain, artifact/signature hashes and all
runtime results.

DONE WHEN: Windows ARM64 has an independently certified release.

DOCUMENTS TO UPDATE: Compatibility matrix, technical anatomy, release evidence,
user manual and plan.

### REL-TASK-045 — macOS release

PHASE: 5  
PRIORITY: P1  
STATUS: FUTURE  
BLOCKED BY: macOS runner/hardware, dependency and Antigravity support, signing
and notarization decisions  
BLOCKS: REL-TASK-047

OBJECTIVE: Build and certify macOS support only for actually supported CPU and
OS versions.

CURRENT EVIDENCE: macOS is in CI matrix/deferred state; no hosted or clean native
macOS certification is current.

IMPLEMENTATION AREA: `apps/desktop/src-tauri/`, `packaging/macos/`,
`packaging/signing/`, `.github/workflows/`, `integrations/`.

IMPLEMENTATION REQUIREMENTS: Packaging, secure storage/keychain, updater,
signing/notarization, runtime integration, installer/update/uninstall and tests.

PRESERVE: No claim from workflow presence alone.

DO NOT: Call macOS supported until a real Mac gate passes.

TESTS REQUIRED: Native macOS fmt/clippy/tests/build, frontend gates, signing,
notarization, clean-machine and runtime integration.

NEW TESTS REQUIRED: Keychain, quarantine, update rollback and unsupported OS/
architecture tests.

MANUAL VERIFICATION: Clean Mac install and real supported runtime journey.

ACCEPTANCE CRITERIA: Stated macOS versions/architectures pass all platform gates.

EVIDENCE TO RECORD: Runner/hardware, OS/architecture, package/signature hashes,
notarization and user journey.

DONE WHEN: macOS support is explicitly scoped and certified.

DOCUMENTS TO UPDATE: Compatibility matrix, release docs, user manual and plan.

### REL-TASK-046 — Linux release

PHASE: 5  
PRIORITY: P1  
STATUS: FUTURE  
BLOCKED BY: Linux distribution scope, packaging/signing, dependency and
Antigravity support, clean machines  
BLOCKS: REL-TASK-047

OBJECTIVE: Certify a defined Linux distribution/package scope rather than
claiming generic Linux support.

CURRENT EVIDENCE: Linux is in CI matrix/deferred state; `packaging/linux/` is a
placeholder and no clean Linux release is certified.

IMPLEMENTATION AREA: `apps/desktop/src-tauri/`, `packaging/linux/`,
`packaging/signing/`, `.github/workflows/`, `integrations/`.

IMPLEMENTATION REQUIREMENTS: Choose distributions/package formats, secure
storage, updater, signing, runtime integration and support lifecycle.

PRESERVE: Capability-based scope and no unsupported distro promise.

DO NOT: Treat Ubuntu CI compile as all-Linux certification.

TESTS REQUIRED: Native Linux build, package/signature/updater, clean-machine and
runtime integration tests.

NEW TESTS REQUIRED: Distro/package compatibility, permissions, secure storage and
rollback tests.

MANUAL VERIFICATION: Clean machines for every supported package/distribution.

ACCEPTANCE CRITERIA: Each stated distribution/package has complete evidence.

EVIDENCE TO RECORD: Distro/version/architecture, package hashes/signatures,
install/update/uninstall and runtime results.

DONE WHEN: Linux support is bounded and certified.

DOCUMENTS TO UPDATE: Compatibility matrix, release docs, user manual and plan.

### REL-TASK-047 — Cross-platform release matrix

PHASE: 5  
PRIORITY: P0  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-043 through REL-TASK-046  
BLOCKS: mature-product release

OBJECTIVE: Maintain one explicit matrix of supported IDE/platform combinations,
with no inherited or implied support.

CURRENT EVIDENCE: CI lists Windows/macOS/Linux, while only local Windows x64
artifacts are current.

IMPLEMENTATION AREA: `.github/workflows/quality.yml`, `packaging/`,
`integrations/`, `tooling/evidence/`, canonical docs.

IMPLEMENTATION REQUIREMENTS: Record OS, architecture, IDE adapter, package,
signing, secure storage, updater and clean-machine status per combination.

PRESERVE: Honest `NOT_RUN`/blocked labels.

DO NOT: Convert a matrix row to supported because a compiler job passed.

TESTS REQUIRED: Every row's platform and adapter gates.

NEW TESTS REQUIRED: Matrix completeness/expiry verifier.

MANUAL VERIFICATION: Independent review of each claimed row.

ACCEPTANCE CRITERIA: Every public support claim maps to a fresh accepted row.

EVIDENCE TO RECORD: Matrix version, row evidence IDs, expiry and owners.

DONE WHEN: Public compatibility claims are generated from accepted evidence.

DOCUMENTS TO UPDATE: AI handoff, technical anatomy, user manual and plan.

## 12. Phase 6 — Advanced Relintor

The following work is intentionally later than production foundations. Items
that are not already established by repository evidence are marked `FUTURE`,
`PLANNED`, or `REQUIRES PRODUCT DECISION`; recommendations are not commitments.

### REL-TASK-048 — Stronger browser/runtime verification

PHASE: 6 — Advanced Relintor  
PRIORITY: P2  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-030, security review and product scope  
BLOCKS: mature evidence automation

OBJECTIVE: Add stronger, bounded verification of browser/runtime behavior where
it materially improves requirement proof.

CURRENT EVIDENCE: Evidence and standards foundations exist; no generic browser
runtime authority is certified.

IMPLEMENTATION AREA: `crates/relintor-evidence/`, `crates/relintor-standards/`,
`crates/relintor-contracts/`, `apps/desktop/src-tauri/src/`.

IMPLEMENTATION REQUIREMENTS: Define trusted collectors, browser/session scope,
artifacts, replay/invalidation and privacy limits.

PRESERVE: Evidence binding and no screenshot-as-authority shortcut.

DO NOT: Treat browser DOM text or AI interpretation alone as proof.

TESTS REQUIRED: Evidence adversarial suites, workspace tests and secret scan.

NEW TESTS REQUIRED: Forged browser state, stale capture, scope escape and
collector identity tests.

MANUAL VERIFICATION: Disposable browser/runtime scenarios.

ACCEPTANCE CRITERIA: New evidence classes are independently bound and fail closed.

EVIDENCE TO RECORD: Collector identity/version, scope, hashes and authority result.

DONE WHEN: Approved runtime evidence class has certification coverage.

DOCUMENTS TO UPDATE: Technical anatomy, standards docs and plan.

### REL-TASK-049 — Richer evidence automation

PHASE: 6  
PRIORITY: P2  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-048  
BLOCKS: REL-TASK-055

OBJECTIVE: Automate collection, dependency graphs, invalidation and report
generation without weakening human/audit authority.

CURRENT EVIDENCE: Evidence graph, fingerprints, traceability and P8 guards exist.

IMPLEMENTATION AREA: `crates/relintor-evidence/`, `crates/relintor-standards/`,
`tooling/acceptance/`, `tooling/evidence/`.

IMPLEMENTATION REQUIREMENTS: Add deterministic collectors, source/environment
fingerprints, evidence expiry and compact reports.

PRESERVE: Criterion binding, revision history and independent verification.

DO NOT: Accept keyword/comment presence or stale output as implementation proof.

TESTS REQUIRED: Existing P6/P8 source and authority guards, workspace tests.

NEW TESTS REQUIRED: Same-size changes, forged collectors, stale evidence and
dependency-cycle cases.

MANUAL VERIFICATION: Run against known-good and seeded-bad repositories.

ACCEPTANCE CRITERIA: Automation reduces toil while preserving all adversarial
rejections.

EVIDENCE TO RECORD: Collector versions, fingerprints, accepted/rejected cases.

DONE WHEN: Automated evidence reports are trusted by the existing verifier.

DOCUMENTS TO UPDATE: Technical anatomy, repository verification and plan.

### REL-TASK-050 — Stronger execution enforcement and isolation

PHASE: 6  
PRIORITY: P0  
STATUS: FUTURE  
BLOCKED BY: REL-TASK-039, platform security capabilities and threat model  
BLOCKS: advanced enterprise workflows

OBJECTIVE: Add stronger sandboxing, process policy and resource isolation around
executor work where the supported platform allows it.

CURRENT EVIDENCE: Process-safety, scope and bounded I/O foundations exist;
complete production sandbox certification does not.

IMPLEMENTATION AREA: `crates/relintor-execution/`,
`crates/relintor-antigravity/`, `apps/desktop/src-tauri/src/`,
`packaging/` and `integrations/`.

IMPLEMENTATION REQUIREMENTS: Threat-model isolation, permissions, cancellation,
network/filesystem policy, resource limits and recovery.

PRESERVE: User-authorized scope and evidence authority.

DO NOT: Promise OS-level isolation unavailable on a target platform.

TESTS REQUIRED: Execution adversarial tests, platform security tests and clean
runtime fixtures.

NEW TESTS REQUIRED: Escape, symlink/reparse, resource exhaustion and kill tests.

MANUAL VERIFICATION: Attack-oriented disposable projects on each supported OS.

ACCEPTANCE CRITERIA: Defined threats are blocked or explicitly scoped with safe
fallback behavior.

EVIDENCE TO RECORD: Policy, platform capability, attack results and residual risk.

DONE WHEN: Isolation is independently security-reviewed for each claim.

DOCUMENTS TO UPDATE: Security report, technical anatomy, compatibility matrix and
plan.

### REL-TASK-051 — External authority and audit export

PHASE: 6  
PRIORITY: P1  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-029, founder audit/retention policy  
BLOCKS: mature enterprise release

OBJECTIVE: Provide verifiable external audit exports or authority handoff when
customers require proof outside the local desktop.

CURRENT EVIDENCE: Local signed evidence, standards distribution and export
primitives exist; external authority protocol is not established.

IMPLEMENTATION AREA: `crates/relintor-evidence/`, `crates/relintor-distribution/`,
`crates/relintor-standards/`, `services/cloud-api/`, `apps/admin/src/`.

IMPLEMENTATION REQUIREMENTS: Define export schema, signatures, identity,
retention, revocation, recipient verification and offline validation.

PRESERVE: Local authority remains explicit; cloud does not invent local facts.

DO NOT: Call a PDF or screenshot an authenticated certificate without binding.

TESTS REQUIRED: Distribution/evidence tests, signature and tamper tests, live
tenant authorization tests.

NEW TESTS REQUIRED: Export tamper, wrong-recipient, expiry and revocation cases.

MANUAL VERIFICATION: Export and verify from an independent environment.

ACCEPTANCE CRITERIA: Recipient can validate origin, scope, time and integrity.

EVIDENCE TO RECORD: Export schema/version, signer identity, hashes and validation.

DONE WHEN: Approved external audit handoff is certified.

DOCUMENTS TO UPDATE: Technical anatomy, admin manual, security and plan.

### REL-TASK-052 — Multi-machine/team recovery

PHASE: 6  
PRIORITY: P1  
STATUS: REQUIRES PRODUCT DECISION  
BLOCKED BY: founder data-sync/recovery policy, REL-TASK-013, REL-TASK-024  
BLOCKS: mature team workflows

OBJECTIVE: Decide and, if approved, implement safe recovery across machines or
team members without silently synchronizing sensitive project state.

CURRENT EVIDENCE: Local-first state and recovery exist; no product commitment to
multi-machine project sync is established.

IMPLEMENTATION AREA: `crates/relintor-execution/`,
`crates/relintor-evidence/`, `services/cloud-api/`, `apps/desktop/src-tauri/src/`.

IMPLEMENTATION REQUIREMENTS: Founder decides what is synced, encrypted, shared,
revoked, conflict-resolved and recoverable.

PRESERVE: Local authority and user consent.

DO NOT: Upload project content by default or infer sync consent.

TESTS REQUIRED: Recovery/evidence/tenant tests after policy approval.

NEW TESTS REQUIRED: Conflict, revocation, partial transfer and stale evidence.

MANUAL VERIFICATION: Two-machine disposable team scenario.

ACCEPTANCE CRITERIA: Approved data model recovers safely with explicit conflicts
and no unauthorized disclosure.

EVIDENCE TO RECORD: Policy, encryption/scope, transfer IDs and authority results.

DONE WHEN: Multi-machine recovery is either certified or explicitly declined.

DOCUMENTS TO UPDATE: Technical anatomy, user/admin manual, decision log and plan.

### REL-TASK-053 — Rich organization policies and audit controls

PHASE: 6  
PRIORITY: P1  
STATUS: PLANNED  
BLOCKED BY: REL-TASK-035, REL-TASK-051, founder policy decisions  
BLOCKS: mature enterprise release

OBJECTIVE: Extend organization/team policies only where customer-approved
controls improve safety and governance.

CURRENT EVIDENCE: Team policies, admin MFA, grants, audit events and entitlement
foundations exist; advanced policy scope is not committed.

IMPLEMENTATION AREA: `services/cloud-api/src/p10.rs`, `services/cloud-api/src/lib.rs`,
`db/migrations/009_p10_teams_billing_admin.sql`, `apps/admin/src/`.

IMPLEMENTATION REQUIREMENTS: Define policy objects, enforcement points,
delegation, approval, audit, export and rollback.

PRESERVE: Server-side authorization and tenant isolation.

DO NOT: Add arbitrary enterprise controls without product decision and threat model.

TESTS REQUIRED: Authorization, admin/MFA, live PostgreSQL and audit tests.

NEW TESTS REQUIRED: Policy conflict, delegation, rollback and cross-tenant cases.

MANUAL VERIFICATION: Admin policy rollout in a disposable organization.

ACCEPTANCE CRITERIA: Each approved policy is enforced consistently and audited.

EVIDENCE TO RECORD: Policy version, actor, scope, affected action and audit IDs.

DONE WHEN: Approved advanced policies are production-certified.

DOCUMENTS TO UPDATE: Admin manual, technical anatomy, plan and decisions.

### REL-TASK-054 — Additional AI provider evaluation and integration

PHASE: 6  
PRIORITY: P2  
STATUS: REQUIRES PRODUCT DECISION  
BLOCKED BY: provider selection, threat model and cost/quality policy  
BLOCKS: only the selected provider's release

OBJECTIVE: Add another AI provider only when it gives approved reliability,
quality, cost or availability value.

CURRENT EVIDENCE: Gateway boundaries include DeepSeek/OpenAI adapter concepts;
no additional provider is committed by current evidence.

IMPLEMENTATION AREA: `services/ai-gateway/src/lib.rs`,
`services/ai-gateway/src/main.rs`, `packages/contracts/src/cloudApi.ts`.

IMPLEMENTATION REQUIREMENTS: Evaluate API semantics, privacy, latency, usage,
failure, model policy, key operations and routing before implementation.

PRESERVE: Server-side keys, bounded deadlines, usage accounting and no AI
completion authority.

DO NOT: Add a provider because a library is available or claim live support from
an adapter enum.

TESTS REQUIRED: Gateway contract, timeout/outage/usage/security tests and secret
scan.

NEW TESTS REQUIRED: Provider-specific compatibility and redaction tests.

MANUAL VERIFICATION: Approved provider sandbox/live smoke after decision.

ACCEPTANCE CRITERIA: Provider passes the same gateway contract and has an
approved support/cost/privacy policy.

EVIDENCE TO RECORD: Decision, provider/model/endpoint class, latency and usage.

DONE WHEN: The selected additional provider is separately certified.

DOCUMENTS TO UPDATE: Technical anatomy, AI operations docs, plan and decision log.

### REL-TASK-055 — Mature Relintor product certification

PHASE: 6  
PRIORITY: P0  
STATUS: FUTURE  
BLOCKED BY: REL-TASK-043, REL-TASK-047, REL-TASK-048 through REL-TASK-054 and
all accepted product decisions  
BLOCKS: none

OBJECTIVE: Issue the mature-product decision only when the supported matrix,
authority, evidence, operations, commercial policy and recovery model are all
freshly certified.

CURRENT EVIDENCE: Repository foundations and multiple source audits exist;
current product is not mature multi-IDE/platform certified.

IMPLEMENTATION AREA: `tooling/evidence/`, `tooling/acceptance/`, all supported
adapter/platform areas and canonical `docs/`.

IMPLEMENTATION REQUIREMENTS: Reconcile every accepted feature, platform/IDE row,
security review, support policy, commercial operation and evidence expiry.

PRESERVE: Product constitution, completion authority, fail-closed semantics and
truthful scope.

DO NOT: Treat feature count or marketing coverage as maturity.

TESTS REQUIRED: Full supported matrix, independent source/runtime audits,
security, commercial, recovery and release gates.

NEW TESTS REQUIRED: Only gaps found by final certification, each with a new task.

MANUAL VERIFICATION: Independent release and product review.

ACCEPTANCE CRITERIA: Every public commitment has current bound evidence and an
owner; every unsupported area is explicitly out of scope.

EVIDENCE TO RECORD: Final certificate inputs, matrix, decisions, artifact hashes,
operational receipts and expiry dates.

DONE WHEN: Mature Relintor certification is accepted without unresolved release-
blocking evidence.

DOCUMENTS TO UPDATE: All canonical docs, support/release docs and this plan.

## 13. Future Feature Register

This register prevents speculative recommendations from becoming commitments.
Task IDs above are the engineering execution records; this table is the product
intent index.

| ID | Name | Reason / user value | Priority | Dependency / likely components | Status | Target phase | Acceptance concept | Open questions |
|---|---|---|---|---|---|---|---|---|
| FEAT-001 | Windows x64 closed Beta | Let real users install and trust the first supported journey. | P0 | REL-TASK-001–010; desktop, website, packaging. | AGREED | 0 | Fresh clean-machine and runtime evidence. | Which Beta cohort and support hours? |
| FEAT-002 | Safe signed updates | Reduce manual upgrade risk. | P0 | REL-TASK-025–027; updater/distribution. | AGREED | 2 | Signed/tampered/rollback tests pass. | Endpoint, channel and rollout policy. |
| FEAT-003 | Production auth and teams | Secure individual and organization access. | P0 | REL-TASK-022–023, 035–036; cloud/admin/desktop. | AGREED | 2–3 | Live tenant/authz evidence. | Final recovery and retention policy. |
| FEAT-004 | Commercial subscriptions | Convert approved access policy into paid product. | P0 | REL-TASK-031–032; cloud/admin/db. | REQUIRES PRODUCT DECISION | 3 | Provider sandbox/live lifecycle and reconciliation. | Provider, prices, taxes, refunds. |
| FEAT-005 | Founder complimentary access | Support founder-approved early access. | P0 | REL-TASK-033; cloud/admin/db. | AGREED | 3 | Scoped, expiring, audited grant. | Eligibility and duration. |
| FEAT-006 | Selected-company grants | Support approved company pilots. | P1 | REL-TASK-034; cloud/admin/db. | AGREED | 3 | Approved terms and revocation. | Selection and quota policy. |
| FEAT-007 | Antigravity integration | First supported executor integration. | P0 | REL-TASK-009, 039–040; adapter/integration. | AGREED | 0–4 | Real discovery, execution, interruption and signed package. | Official bridge distribution path. |
| FEAT-008 | IDE-agnostic adapter model | Prevent Relintor identity from being tied to one IDE. | P0 | REL-TASK-039, 043; contracts/execution. | AGREED | 4 | Common adapter conformance suite. | Versioning/support policy. |
| FEAT-009 | Additional IDE adapter | Expand customer choice after demand validation. | P1 | REL-TASK-041–043. | REQUIRES PRODUCT DECISION | 4 | Selected adapter independently certified. | Which IDE and order? |
| FEAT-010 | Windows ARM64 | Support compatible Windows ARM devices. | P1 | REL-TASK-044; packaging/desktop/adapter. | AGREED | 5 | Real ARM64 clean-machine matrix. | Antigravity/dependency support. |
| FEAT-011 | macOS distribution | Support an approved Mac scope. | P1 | REL-TASK-045; packaging/signing/keychain. | AGREED | 5 | Real Mac package/sign/runtime evidence. | CPU/OS scope and launch order. |
| FEAT-012 | Linux distribution | Support an approved Linux scope. | P1 | REL-TASK-046; packaging/signing. | AGREED | 5 | Per-distro clean-machine evidence. | Distro/package order. |
| FEAT-013 | Stronger evidence automation | Reduce verification toil while increasing proof quality. | P2 | REL-TASK-048–049; evidence/standards/tooling. | PLANNED | 6 | Adversarial stale/forged evidence tests. | Which collectors are trusted? |
| FEAT-014 | Execution isolation | Reduce impact of unsafe executor behavior. | P0 | REL-TASK-050; execution/platform security. | PLANNED | 6 | Threat-model and platform test coverage. | Required isolation level by OS. |
| FEAT-015 | External audit exports | Let customers independently validate completion. | P1 | REL-TASK-051; evidence/distribution/cloud/admin. | PLANNED | 6 | Independent signature/integrity validation. | Recipient/retention policy. |
| FEAT-016 | Multi-machine recovery | Support approved team continuity. | P1 | REL-TASK-052; recovery/cloud/evidence. | REQUIRES PRODUCT DECISION | 6 | Explicit encrypted transfer/conflict behavior. | What project data may sync? |
| FEAT-017 | Advanced org controls | Serve governed teams and enterprise workflows. | P1 | REL-TASK-053; cloud/admin/db. | PLANNED | 6 | Per-policy authz/audit tests. | Which controls are worth complexity? |
| FEAT-018 | Additional AI providers | Improve availability/quality/cost where justified. | P2 | REL-TASK-054; AI gateway/contracts. | REQUIRES PRODUCT DECISION | 6 | Same provider contract and policy evidence. | Which provider and why? |

## 14. Product Decisions Required From Founder

An AI may collect options and evidence but must not silently decide these items.

| Decision | When blocking | Options already established | Default if any | Why AI must not silently choose |
|---|---|---|---|---|
| Billing provider | Before REL-TASK-032 or paid release. | Existing billing/entitlement API foundation; provider not selected. | None. | Fees, tax, compliance, payout and lock-in are business decisions. |
| Pricing, trials, refunds and grace | Before REL-TASK-032. | Entitlement/grace mechanisms exist in source. | None. | Directly changes customer obligations and access. |
| Founder complimentary eligibility/duration | Before REL-TASK-033. | Audited grant foundation exists. | None. | Creates financial and fairness commitments. |
| Selected-company grant policy | Before REL-TASK-034. | Company grant foundation exists. | None. | Determines who receives commercial value and under what terms. |
| Team/seat/ownership policy | Before REL-TASK-035. | Organization, membership and team policy foundations exist. | Least privilege until decided. | Changes customer governance and data access. |
| Data retention/export/deletion | Before REL-TASK-037 and production. | Local-first and cloud data boundaries are documented. | Preserve data until policy is approved; do not invent retention. | Legal/privacy impact. |
| Support SLA and launch cohort | Before REL-TASK-038. | Closed Beta surface exists. | Controlled cohort only. | Determines operational promise and staffing. |
| Permanent domain/deployment order | Before REL-TASK-020. | Local/staging endpoint names exist. | No public production claim. | Infrastructure cost and public trust. |
| Updater channel/rollout policy | Before REL-TASK-026. | Tauri updater boundary and fail-closed design exist. | Disabled/unavailable until configured. | Release risk and rollback obligations. |
| Next IDE support order | Before REL-TASK-041. | Antigravity is first; core must stay IDE-neutral. | No additional promise. | Demand, licensing and vendor support. |
| Platform launch order and exact OS scope | Before REL-TASK-044–046. | Windows x64 is current; others are deferred. | Windows x64 only. | Hardware, support and certification cost. |
| Multi-machine project sync | Before REL-TASK-052. | Local-first architecture exists. | No sync/upload by default. | Sensitive project-data and consent implications. |
| Additional AI provider | Before REL-TASK-054. | Gateway has provider boundary; no new provider committed. | Existing approved provider only. | Privacy, quality, cost and operations. |
| Public launch criteria | Before REL-TASK-030/038/055. | Phase exit criteria in this plan. | No public certification until all required gates pass. | Founder owns risk appetite and market promise. |

## 15. Release Gates

### Beta release

REL-TASK-001–010 pass; Windows x64 only; clean-machine install/launch/use/
uninstall evidence; live dependencies that Beta promises are real; website hash
matches installer; security/traceability/spec gates pass; macOS/Linux/ARM64 and
unconfigured updater remain explicitly not supported or unavailable.

### Stabilized Beta

REL-TASK-011–019 pass; three repeat runs, recovery and diagnostics evidence,
stable disposable environment, no open P0/P1 reliability/security defect.

### Controlled paid release

REL-TASK-020–038 pass as applicable; founder decisions recorded; billing,
entitlement, grants, support, privacy, rollback and live operations reconcile.

### Public Windows release

Windows x64 signing, provenance, updater/release policy, clean-machine matrix,
security review, support and incident response pass. Public copy names only
certified Windows x64 scope.

### Each additional platform

The corresponding platform task (044, 045 or 046), adapter task 043, signing,
secure storage, updater, packaging, clean-machine and failure/recovery gates pass
for each stated OS/architecture. No cross-platform inference is allowed.

### Each additional IDE integration

REL-TASK-039, the selected IDE decision, adapter implementation and REL-TASK-043
pass with separate compatibility, package/signature, discovery, execution,
interruption, recovery, uninstall and unsupported-state evidence.

## 16. Current Execution Pointer

```text
CURRENT_TASK_ID: REL-TASK-001
CURRENT_PHASE: 0 — Close Current Windows x64 Beta
CURRENT_OBJECTIVE: Add and run the updater configuration regression test.
CURRENT_BLOCKERS: Rust 1.96 x64 MSVC environment is not installed in the current runner; clean-machine and live external gates remain unrun; website dist is stale; PROJECT_CONTEXT_TRANSFER.md is unavailable as a filesystem file.
NEXT_TASK_ID: REL-TASK-002
LAST_VERIFIED_DATE: 2026-08-18
LAST_VERIFIED_COMMIT: unavailable (repository has no usable committed HEAD in the current checkout)
```

## 17. Maintenance Protocol

When a task is completed:

1. Verify it against its acceptance criteria.
2. Record exact commands, exit codes, artifacts, hashes, runtime identity and
   raw failure evidence.
3. Change only that task's status after evidence is accepted.
4. Update `CURRENT_TASK_ID`, `CURRENT_PHASE`, blockers and next task.
5. Update the affected canonical docs and milestone evidence.
6. Add newly discovered work with a new `REL-TASK-*` ID; preserve completed
   records and historical evidence.
7. Preserve `spec/locked`, lockfiles and authority/security boundaries.
8. Commit documentation with implementation when the repository has an approved
   commit workflow; do not touch GitHub without explicit authorization.

## Final Self-Audit

- Current implementation facts come from the repository verification and source,
  not stale transfer claims.
- All known current Beta blockers have Phase 0 tasks.
- Stabilization, production infrastructure, security, commercial, IDE and
  platform work each have separate phases and dependencies.
- Speculative IDEs, providers, pricing, domains, sync and legal policy are not
  silently committed; they are decisions or future work.
- Task implementation areas use verified existing repository locations; future
  adapter files are explicitly not invented.
- Every phase has measurable exit criteria and every task has evidence and
  documentation requirements.

