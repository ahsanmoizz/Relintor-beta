# Relintor P12 Adversarial Release Certification

## Canonical current state

**Certification disposition: `REVALIDATION_REQUIRED`**

This is the single current P12 report. P0-P11 evidence is historical input to
this certification; it is not silently upgraded by this report. P12 found
genuine implementation gaps in the frozen product surface, so the release path
stops at revalidation. P12 did not add product features, modify the sealed
specification, change traceability, touch GitHub, or start a later phase.

The repository has strong local fail-closed, evidence-integrity, security,
standards, orchestration, recovery, desktop, and Windows build evidence. That
evidence is not sufficient to certify a release while the missing production
update/uninstall paths and the production Antigravity integration remain
unimplemented. External runtime gates are recorded separately and are not
converted into local PASS claims.

## Authority and preservation checks

The P12 authority sources were read before execution:

- `spec/locked/RELINTOR_MASTER_SPEC.md`
- `spec/locked/08_FEATURE_REGISTER_144.md`
- `spec/locked/09_IMPLEMENTATION_PHASES.md`
- `spec/locked/10_VERIFICATION_AND_RELEASE_GATES.md`
- the canonical P1-P11 evidence reports
- `tooling/acceptance/implementation-traceability.json`

The sealed specification was not modified. `pnpm-lock.yaml` was not modified.
The traceability file was not modified. GitHub and any remote were not touched.

| Artifact | SHA-256 |
|---|---|
| `spec/locked/RELINTOR_MASTER_SPEC.md` | `D7413BA40FE30D67B2C2768829385DB26118E99B92DACC1FD50C54517A16F194` |
| `spec/locked/08_FEATURE_REGISTER_144.md` | `41FEAB5BC7760127E5DEA875B7E65958A91296D7BF1D7E76D29AB045583AA78C` |
| `spec/locked/09_IMPLEMENTATION_PHASES.md` | `4C8A7F67F2D2A9CE9700BF1B733908BB8B23A43E05A43054D8F93171C5971F3A` |
| `spec/locked/10_VERIFICATION_AND_RELEASE_GATES.md` | `8A78F70FA1CF267DBC5243A1D14B6E30B8EF757581CB98BA9707BEB8569FD8DB` |
| `Cargo.lock` | `A77998501AA417BDE50065D2C5392F9A22954E575FF7B652A5D0B01197FB8C8C` |
| `pnpm-lock.yaml` | `88648E63927643876A2C82EB7D43977C939589782CAB7D7100E829AF5F086D23` |

## Feature audit: all 144 IDs

`python tooling/spec/verify_spec.py` found `files=18`,
`manifest_entries=17`, `features=144`, and `unique_ids=144`. Structural
traceability verification passed for all 144 records. The current source
truthfulness counts are:

| Implementation status | Count |
|---|---:|
| `implemented` | 64 |
| `partially_implemented` | 57 |
| `foundation_only` | 13 |
| `not_started` | 10 |
| **Total** | **144** |

| Verification status | Count |
|---|---:|
| `passed` | 116 |
| `not_started` | 17 |
| `not_run` | 10 |
| `blocked_environment` | 1 |
| **Total** | **144** |

### Release-stopping implementation gaps

These are genuine missing product behaviors, not merely unavailable external
evidence. They require repair in their owning phase and a subsequent
revalidation; P12 does not implement them.

| Feature(s) | Owning phase | Missing behavior | Required repair before release certification |
|---|---|---|---|
| A-08 | P1 | Automatic signed updates are not implemented as a supported product path. | Implement the updater path with signed manifest/artifact verification, version/rollback and downgrade policy, atomic update behavior, failure recovery, and executable tests against tamper, replay, downgrade, and interrupted update cases. |
| A-12 | P1 | Uninstall does not provide the required evidence-preservation/export choice. | Implement a supported uninstall/export/preserve flow with explicit user choice, evidence integrity and secret handling, restore validation, and destructive-action safeguards. |
| F-01..F-12 | P3 | Production Antigravity plugin installation, hook integration, headless execution, exact action binding, permission decisions, authority leases, transcript/artifact capture, worktree/subagent awareness, and adapter compatibility certification are not implemented. The source contains safe boundary primitives and `MockAdapter`, while the bridge manifest is documented-only and unsigned. | Implement the actual signed production plugin/adapter boundary, pre/post/stop hooks, supported headless runner, structured packet/action binding, lease enforcement, bounded transcript/artifact collection, worktree/subagent handling, compatibility matrix, and real runtime tests. Do not substitute `MockAdapter` for production evidence. |

Other conservative records remain truthful rather than being promoted:
A-02, A-04, A-09, A-10, A-11, and L-04..L-07 have source foundations but are
not release-cleared because product integration and/or external runtime/provider
evidence is incomplete. K-09..K-12 likewise remain partial pending live
account/provider and release evidence. No traceability verification status was
restored merely because a local unit test passed.

## Command ledger

All commands below were executed from `D:\Relintor` with the D-first toolchain:

```text
RUSTUP_HOME=D:\Relintor-rustup
CARGO_HOME=D:\Relintor-cargo-home
CARGO_TARGET_DIR=D:\Relintor\target
TEMP=D:\Relintor-temp
TMP=D:\Relintor-temp
Visual Studio environment: D:\DevTools\VSBuildTools\Common7\Tools\VsDevCmd.bat -arch=x64 -host_arch=x64
Rust: 1.96.0, host x86_64-pc-windows-msvc
cl.exe: D:\DevTools\VSBuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\cl.exe
link.exe: D:\DevTools\VSBuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\link.exe
```

| Exact command | Exit code | Result | Notes |
|---|---:|---|---|
| `cargo fmt --all -- --check` | 0 | PASS | x64 MSVC developer environment |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | PASS | No warnings promoted to errors |
| `cargo test --workspace --locked` | 0 | PASS | 263 passed, 0 failed, 2 ignored |
| `python tooling/spec/verify_spec.py` | 0 | PASS | 144 unique feature IDs |
| `python tooling/acceptance/test_spec_verifier.py` | 0 | PASS | Baseline and mutation cases behaved as expected |
| `python tooling/acceptance/verify_traceability.py` | 0 | PASS | 144/144 records structurally accounted for |
| `python tooling/acceptance/secret_scan.py` | 0 | PASS | No provider/private-key/connection-secret matches |
| `pnpm install --frozen-lockfile --store-dir D:\Relintor-pnpm-store` | 0 | PASS | Frozen lockfile preserved |
| `pnpm --dir apps/desktop typecheck` | 0 | PASS | |
| `pnpm --dir apps/desktop lint` | 0 | PASS | |
| `pnpm --dir apps/desktop test` | 0 | PASS | 10 tests; React act warnings only |
| `pnpm --dir apps/desktop build` | 0 | PASS | |
| `pnpm --dir apps/admin typecheck` | 0 | PASS | |
| `pnpm --dir apps/admin lint` | 0 | PASS | |
| `pnpm --dir apps/admin test` | 0 | PASS | 6 tests |
| `pnpm --dir apps/admin build` | 0 | PASS | |
| `pnpm --dir apps/website typecheck` | 0 | PASS | |
| `pnpm --dir apps/website lint` | 0 | PASS | |
| `pnpm --dir apps/website test` | 0 | PASS | 2 tests |
| `pnpm --dir apps/website build` | 0 | PASS | |
| `pnpm --dir apps/desktop tauri build --no-bundle` | 0 | PASS | Native x64 MSVC executable |
| `pnpm --dir apps/desktop tauri build --bundles nsis` | 0 | PASS | NSIS installer built |

The workspace totals are the actual Cargo result: **263 passed, 0 failed,
2 ignored**. The ignored tests are the P10 cloud API PostgreSQL round trip and
the cloud API mandatory live PostgreSQL migration/account round trip; both
require `RELINTOR_TEST_DATABASE_URL` and no such URL was present.

## P12 adversarial and carried-gate evidence

### P3 source audit

The P3 boundary source inspection passed for the implemented safety primitives:
workspace path scope, structured process invocation, supported CLI/headless
shape, structured events, stop/restart handling, unavailable fail-closed
behavior, test isolation, and synthetic-success rejection. Renderer inspection
found no completion authority; completion is rendered from persisted backend
state.

The audit also confirmed the release-stopping P3 gap: the Antigravity bridge
manifest says `hook_protocol=documented-only` and `signing_status=not_implemented`,
and the Rust production adapter surface contains `MockAdapter` but no real
signed production adapter. Therefore the source-safety portion is PASS, while
full P3 production integration remains revalidation-required and the existing
external/runtime debt remains unchanged.

### P4/P5 source and runtime audits

The previously executed independent P4 and P5 suites remain PASS. The P5
source audit and runtime acceptance covered forged read-only probes, disposable
sandbox execution of seeded failures, full-file same-size fingerprint changes,
workspace dependency edges and missing references, migration/schema-object
correlation, deterministic-dead-code classification, comment-only auth words,
and the required takeover corpus.

| Suite | Result |
|---|---:|
| `cargo test -p relintor-takeover --test independent_p5_acceptance --locked -- --nocapture` | 5 passed, 0 failed |
| `cargo test -p relintor-takeover --test independent_p5_source_audit --locked -- --nocapture` | 7 passed, 0 failed |
| `cargo test -p relintor-investigator --locked -- --nocapture` | 10 unit + 9 P4 acceptance passed |

### P6 standards and requirement graph

The 18 standards packs, applicability engine, DAG/cycle checks, signed
registry/seal, tamper/revalidation checks, human-defer accounting, and final
source closure checks passed locally:

| Suite | Result |
|---|---:|
| `independent_p6_acceptance` | 13 passed, 0 failed |
| `independent_p6_source_audit` | 10 passed, 0 failed |
| `independent_p6_final_reaudit` | 3 passed, 0 failed |
| `independent_p6_closure_audit` | 1 passed, 0 failed |

### P7 orchestration and watchdog

The execution acceptance/source-closure tests passed: 24 + 6 + 2 tests,
covering dependency ordering, leases, budgets, bounded retries, repeated
failures, no-progress and oscillation stops, external mutation, continuation,
and caller-forged success rejection. No real Antigravity runtime was available.

### P8 completion/evidence authority

The P8 acceptance corpus and final guards passed: 47 + 3 + 1 + 2 + 1 + 1 + 2
tests. These tests reject missing/failed/skipped evidence, stale or tampered
artifacts, invalid bindings, generic test claims, incomplete reports, mutable
workspace authority, and unsupported completion certificates. They do not
constitute a 0% false-done measurement because no sealed external false-done
benchmark corpus exists in this workspace.

### P9 interruption and recovery

`p9_recovery_acceptance` passed **41/41**. Checkpoints, authenticated chains,
compensation safety, process identity, session interruption, path traversal,
secret exclusion, same-size fingerprint changes, and safe incomplete resume
were exercised. The Windows symlink/reparse smoke reported
`P9_WINDOWS_SYMLINK_REPARSE_SMOKE=NOT_RUN` when creation was unavailable; it is
not claimed as PASS. Real Antigravity crash recovery and OS reboot smoke were
not executed.

### AI, billing, and entitlement abuse

The local AI gateway suite passed **9/9**, including a real response deadline,
provider outage normalization, arbitrary bearer rejection, non-mock fail-closed
configuration, secret redaction, request-ID hardening, and usage-budget
enforcement. The cloud API suite passed **18/18** with **2 ignored** live
PostgreSQL tests. Contracts passed **8/8**. Local tests cover founder and
complimentary grants, MFA/session-bound replay rejection, tenant scoping,
seat limits, expired trials, signed entitlement claims, and cross-tenant grant
denial. No real provider, billing account, or PostgreSQL instance was used.

### Scope, loop, and evidence integrity

The P5/P7/P8/P9 corpora passed their local scope, sandbox, loop/waste,
interruption, evidence-integrity, and authority-binding checks. No completion
claim was minted by P12. A sealed false-done benchmark was not present, so its
execution and rate are `NOT_RUN / BLOCKED_EXTERNAL`, not PASS.

## PostgreSQL and external runtime gates

The environment probe found no `RELINTOR_TEST_DATABASE_URL`, `psql`, `pg_ctl`,
`postgres` executable, or PostgreSQL service. The live command was therefore
not run against an unknown or non-disposable database:

```text
cargo test -p relintor-cloud-api mandatory_live_postgres_migration_and_account_round_trip --locked -- --ignored --nocapture
```

Result: `BLOCKED_EXTERNAL` — no disposable local PostgreSQL environment was
available. Consequently migrations 002/003 and the live account/org/device/
session, refresh rotation, policy retrieval, and organization-bound entitlement
round trip are not certified here.

```text
P2_LIVE_POSTGRES_INTEGRATION=PENDING_EXTERNAL_ENVIRONMENT
P10_LIVE_POSTGRES_INTEGRATION=PENDING_EXTERNAL_ENVIRONMENT
P10_REAL_BILLING_PROVIDER=PENDING_EXTERNAL_ENVIRONMENT
```

No current or older Antigravity runtime, browser runtime, independent AI
provider, hosted macOS/Linux runner, or hosted distribution/rollout environment
was available. These remain external blockers, not fabricated local results.

## Security and distribution matrix

| Gate | Result |
|---|---|
| Rust fmt/clippy | PASS |
| Python specification, traceability, and secret checks | PASS |
| Renderer secret scans for desktop/admin/website | PASS |
| Gitleaks 8.30.1 | PASS in all 16 P12 scopes; no leaks found |
| Native x64 MSVC executable | PASS; unsigned |
| NSIS installer | PASS; unsigned |
| Production code signing/notarization | NOT CERTIFIED |
| Signed plugin/adapter packaging | NOT IMPLEMENTED; revalidation required |
| Actual updater/rollback release path | NOT IMPLEMENTED; A-08 revalidation required |
| Actual uninstall/export/preserve path | NOT IMPLEMENTED; A-12 revalidation required |

Gitleaks executable:
`D:\Relintor-cargo-home\gitleaks-tools\gitleaks-8.30.1\gitleaks.exe`.
All 16 scopes passed: `.github`, `apps/desktop`, `apps/admin`,
`apps/website`, `crates`, `db`, `integrations`, `packages`, `services`,
`tooling`, `packaging`, `Cargo.toml`, `package.json`, `pnpm-workspace.yaml`,
`rust-toolchain.toml`, and `REPAIR_MANIFEST.json`.

### Windows local artifacts

| Artifact | Size | SHA-256 | Authenticode |
|---|---:|---|---|
| `target/release/relintor-desktop.exe` | 15,417,344 bytes | `A785A8CA99FB6B20861597AD07EFF999BB57895CAE7F10FA643A82C8F76606AE` | `NotSigned` |
| `target/release/bundle/nsis/Relintor_0.1.0_x64-setup.exe` | 3,781,327 bytes | `0CFC5BF319011F1A5B2880F5BCDA72DB3FF6328C584F719C64889150B2C4F041` | `NotSigned` |

| Platform/gate | Status |
|---|---|
| Windows x64 MSVC compile, tests, native build, installer | PASS for executed local gates |
| Windows clean install/upgrade/uninstall | NOT_RUN; no isolated disposable release environment, no destructive workstation action |
| macOS | `NOT_RUN_CROSS_PLATFORM_GATE_DEFERRED` |
| Linux | `NOT_RUN_CROSS_PLATFORM_GATE_DEFERRED` |
| Human non-expert usability | `NOT_RUN` |

## Exact P12 carried debt ledger

These values are carried unchanged from the previous milestone evidence. The
P12 source findings above do not erase, merge, or upgrade these entries:

```text
P2_LIVE_POSTGRES_INTEGRATION=PENDING_EXTERNAL_ENVIRONMENT
P3_REAL_ANTIGRAVITY_SMOKE=PENDING_EXTERNAL_ENVIRONMENT
P3_PLUGIN_SIGNING_PACKAGING=DEFERRED_TO_RELEASE_GATE
P3_INDEPENDENT_SOURCE_AUDIT=NOT_RUN
P7_REAL_ANTIGRAVITY_END_TO_END=PENDING_EXTERNAL_ENVIRONMENT
P8_REAL_INDEPENDENT_AI_VERIFIER=PENDING_EXTERNAL_ENVIRONMENT
P8_REAL_BROWSER_RUNTIME_SMOKE=PENDING_EXTERNAL_ENVIRONMENT
P9_REAL_ANTIGRAVITY_CRASH_RECOVERY=PENDING_EXTERNAL_ENVIRONMENT
P9_REAL_OS_REBOOT_SMOKE=PENDING_EXTERNAL_ENVIRONMENT
P9_WINDOWS_SYMLINK_REPARSE_SMOKE=NOT_RUN
P10_LIVE_POSTGRES_INTEGRATION=PENDING_EXTERNAL_ENVIRONMENT
P10_REAL_BILLING_PROVIDER=PENDING_EXTERNAL_ENVIRONMENT
P11_REAL_NON_EXPERT_USABILITY=NOT_RUN
P11_L11_LIVE_DISTRIBUTION=PENDING_EXTERNAL_ENVIRONMENT
P11_L12_LIVE_ROLLOUT=PENDING_EXTERNAL_ENVIRONMENT
macOS=NOT_RUN_CROSS_PLATFORM_GATE_DEFERRED
Linux=NOT_RUN_CROSS_PLATFORM_GATE_DEFERRED
```

P12 additionally executed a local P3 boundary source inspection and recorded
its result as `PASS_WITH_REVALIDATION_REQUIRED`; that result is not used to
rewrite the carried ledger entry because the production plugin/adapter boundary
is still absent.

## Storage and changed files

| Drive | Free space after P12 testing |
|---|---:|
| C: | 2,931,875,840 bytes (2.73 GB) |
| D: | 45,453,893,632 bytes (42.33 GB) |

P12 changed exactly one file:

- `tooling/evidence/milestone-12-release-certification.md`

No source files, `spec/locked`, `pnpm-lock.yaml`, `Cargo.lock`, or traceability
records were changed by P12. No completion certificate was generated. The
remaining blockers are the genuine A-08/A-12/F-01..F-12 implementation gaps,
plus the separately recorded external PostgreSQL, provider, runtime,
cross-platform, signing, distribution, and usability gates.

## Final disposition

`REVALIDATION_REQUIRED`

No P13/P14 or other later phase was started.
