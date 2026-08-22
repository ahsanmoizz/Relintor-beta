# Relintor P12 Revalidation Implementation Repair

## Disposition

REVALIDATION_REQUIRED

This is the canonical repair-era report for the targeted P12 gaps A-08, A-12,
and F-01 through F-12. It is not a P12 certification, does not rewrite the
previous P12 certification report, and does not start P13 or any later phase.
GitHub was not accessed or modified. spec/locked, pnpm-lock.yaml, and all
carried P2/P3/P7-P11 debt were preserved.

## D:-first MSVC evidence

| Item | Value |
|---|---|
| Repository | D:\Relintor |
| Toolchain | 1.96.0-x86_64-pc-windows-msvc |
| rustc | 1.96.0 (ac68faa20 2026-05-25) |
| Rust host | x86_64-pc-windows-msvc |
| VS environment | D:\DevTools\VSBuildTools\Common7\Tools\VsDevCmd.bat -arch=x64 -host_arch=x64 |
| cl.exe | D:\DevTools\VSBuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\cl.exe |
| link.exe | D:\DevTools\VSBuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\link.exe |
| RUSTUP_HOME | D:\Relintor-rustup |
| CARGO_HOME | D:\Relintor-cargo-home |
| CARGO_TARGET_DIR | D:\Relintor\target |
| TEMP/TMP | D:\Relintor-temp |

## Repaired implementation

### A-08 — signed application updates

crates/relintor-distribution now provides Ed25519 signature and signer identity
verification, product/target/version binding, validity-window checks, exact
artifact SHA-256 verification, replay/downgrade/stale rejection, bounded
artifacts, and atomic verified staging with read-back integrity checks.

The Tauri desktop boundary now exposes explicit update_check and update_apply
commands through the updater plugin. Renderer input cannot mint trust, signer
identity, digest, or success. Missing endpoint/public-key configuration returns
an external-blocked result. Production signing keys are not present in source,
renderer, bundle, database, logs, or tests.

Focused evidence: distribution unit tests 10 passed; integration test
p12_revalidation_acceptance 3 passed. The integration matrix exercised valid
signature, wrong signer, manifest/artifact tamper, digest mismatch, replay,
downgrade, wrong target, stale metadata, and real update staging. Production
update signing and the live update service remain external.

### A-12 — uninstall and evidence preservation

The distribution boundary requires an explicit PRESERVE,
EXPORT_THEN_REMOVE, or REMOVE_ALLOWED_STATE choice. Export is allowlisted,
identity-bound, deterministic, integrity-authenticated, secret-filtered,
bounded, symlink/reparse-safe, and committed through temporary write/read-back
verification. Removal is limited to supported Relintor state and cannot report
success after a failed or unverified export.

The same 10 unit and 3 integration tests covered valid export/read-back, tamper,
truncation, wrong identity, unsafe paths, secret exclusion, preserve,
export-then-remove, remove-allowed-state, and failure-before-success.

### F-01 through F-12

| ID | Production repair | Evidence |
|---|---|---|
| F-01 | Signed plugin identity/version/manifest/file digests, path and symlink checks, atomic install/replacement, compatibility metadata | Antigravity plugin-install test; external signing required for deployment |
| F-02 | Structured pre-tool interception with trusted execution identity and exact action validation | Production hook test |
| F-03 | Post-tool result and artifact capture bound to the authorized action | Production hook/output tests |
| F-04 | Stop event records stop/requested-stop and never means completion | Hook and execution state tests |
| F-05 | Real executable discovery, structured headless invocation, minimized environment, bounded subprocess, timeout/cancel, identity, output, and exit parsing | Production adapter path; missing runtime fails closed |
| F-06 | Mission/revision/seal/task/project/workspace/scope/tools/budgets/expiry/evidence/lease task packet | Packet and execution regression tests |
| F-07 | Canonical packet/action digests reject mutation, replay, path, task, or worktree mismatch | Packet and hook binding tests |
| F-08 | Relintor-owned low-risk policy maps to explicit native allow/deny; unknown/stale input denies | Pre-tool hook test |
| F-09 | Lease identity and expiry enforced before production execution | P7 lease regression suite |
| F-10 | Actual process result, bounded/redacted transcripts, events, artifacts, and process identity | Production adapter implementation/tests |
| F-11 | Canonical worktree identity and explicit subagent field prevent authority leakage | Worktree packet/hook binding tests |
| F-12 | Installed/supported/unsupported/unknown/not-installed matrix records actual version/protocol/runner/hooks | Compatibility tests |

ProductionAdapter::new() is the default production path. It does not fall
back to MockAdapter; it requires a supported runtime and externally installed,
hash-verified bridge manifest. MockAdapter remains test-only.

## Rust and focused command ledger

| Exact command | Exit | Result |
|---|---:|---|
| cargo fmt --all | 0 | PASS |
| cargo fmt --all -- --check | 0 | PASS |
| cargo test -p relintor-antigravity -p relintor-distribution --locked -- --nocapture | 0 | PASS — 12 Antigravity, 10 distribution, 3 integration |
| cargo test -p relintor-execution --locked -- --nocapture | 0 | PASS — 73 P7/P9 tests |
| cargo clippy --workspace --all-targets --locked -- -D warnings | 0 | PASS |
| cargo test --workspace --locked | 0 | PASS — 279 passed, 0 failed, 2 ignored |

The two ignored workspace tests require RELINTOR_TEST_DATABASE_URL; no live
PostgreSQL certification is claimed here.

## Specification, traceability, and dependency ledger

| Exact command | Exit | Result |
|---|---:|---|
| python tooling/spec/verify_spec.py | 0 | PASS — 18 files, 144 unique IDs |
| python tooling/acceptance/test_spec_verifier.py | 0 | PASS — baseline and mutation cases |
| python tooling/acceptance/verify_traceability.py | 0 | PASS — 144 records |
| python tooling/acceptance/secret_scan.py | 0 | PASS |
| pnpm install --frozen-lockfile --store-dir D:\Relintor-pnpm-store | 0 | PASS |

Affected records only:

A-08, A-12, F-01..F-12: implementation_status=implemented,
verification_status=passed

No unrelated record was upgraded.

## Desktop, admin, website, native, and installer ledger

| Exact command/group | Exit | Result |
|---|---:|---|
| pnpm --dir apps/desktop typecheck | 0 | PASS |
| pnpm --dir apps/desktop lint | 0 | PASS |
| pnpm --dir apps/desktop test | 0 | PASS — 10 tests |
| pnpm --dir apps/desktop build | 0 | PASS |
| pnpm --dir apps/admin typecheck | 0 | PASS |
| pnpm --dir apps/admin lint | 0 | PASS |
| pnpm --dir apps/admin test | 0 | PASS — 6 tests |
| pnpm --dir apps/admin build | 0 | PASS |
| pnpm --dir apps/website typecheck | 0 | PASS |
| pnpm --dir apps/website lint | 0 | PASS |
| pnpm --dir apps/website test | 0 | PASS — 2 tests |
| pnpm --dir apps/website build | 0 | PASS |
| pnpm --dir apps/desktop tauri build --no-bundle | 0 | PASS — MSVC release executable |
| pnpm --dir apps/desktop tauri build --bundles nsis | 0 | PASS — NSIS installer |

Final artifacts:

D:\Relintor\target\release\relintor-desktop.exe
size=19461632
sha256=FD212153B1074B45FBC8A555601EE6B22BD5772331D2D505C9DAA02E064CD14E

D:\Relintor\target\release\bundle\nsis\Relintor_0.1.0_x64-setup.exe
size=4971299
sha256=1231B9C427B25A5984F488D95357798EE2CB1169C94A713F912B4B5A5B9E18E4

Both are local unsigned artifacts; production desktop/updater signing remains
external.

## Gitleaks and renderer scans

Executable: D:\Relintor-cargo-home\gitleaks-tools\gitleaks-8.30.1\gitleaks.exe.
gitleaks version exited 0 and reported 8.30.1. Each scope used:

D:\Relintor-cargo-home\gitleaks-tools\gitleaks-8.30.1\gitleaks.exe dir --redact --no-banner --exit-code 1 SCOPE

All 16 scopes exited 0 with no leaks found:

.github PASS; apps/desktop PASS; apps/admin PASS; apps/website PASS;
crates PASS; db PASS; integrations PASS; packages PASS; services PASS;
tooling PASS; packaging PASS; Cargo.toml PASS; package.json PASS;
pnpm-workspace.yaml PASS; rust-toolchain.toml PASS; REPAIR_MANIFEST.json PASS.

Renderer scans all exited 0:

python tooling/acceptance/secret_scan.py --root apps/desktop
python tooling/acceptance/secret_scan.py --root apps/admin
python tooling/acceptance/secret_scan.py --root apps/website

## Runtime and carried debt

where.exe agy exited 1 with INFO: Could not find files for the given
pattern(s). No live Antigravity smoke ran and no MockAdapter result was used
as a substitute.

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

## Locks and sealed specifications

Cargo.lock was legitimately regenerated for the new distribution and updater
dependencies:

Cargo.lock 668CB94103A8C8F9FD01C5983EC1FE2796C294D349B866F4A32C34B8153FAD95
pnpm-lock.yaml 88648E63927643876A2C82EB7D43977C939589782CAB7D7100E829AF5F086D23 (unchanged)

spec/locked is unchanged. Sealed hashes are:

RELINTOR_MASTER_SPEC.md D7413BA40FE30D67B2C2768829385DB26118E99B92DACC1FD50C54517A16F194
08_FEATURE_REGISTER_144.md 41FEAB5BC7760127E5DEA875B7E65958A91296D7BF1D7E76D29AB045583AA78C
09_IMPLEMENTATION_PHASES.md 4C8A7F67F2D2A9CE9700BF1B733908BB8B23A43E05A43054D8F93171C5971F3A
10_VERIFICATION_AND_RELEASE_GATES.md 8A78F70FA1CF267DBC5243A1D14B6E30B8EF757581CB98BA9707BEB8569FD8DB

## Files changed

Cargo.toml
Cargo.lock
crates/relintor-distribution/Cargo.toml
crates/relintor-distribution/src/lib.rs
crates/relintor-distribution/tests/p12_revalidation_acceptance.rs
crates/relintor-antigravity/Cargo.toml
crates/relintor-antigravity/src/lib.rs
crates/relintor-execution/src/lib.rs
apps/desktop/src-tauri/Cargo.toml
apps/desktop/src-tauri/src/lib.rs
apps/desktop/src-tauri/tauri.conf.json
integrations/antigravity/compatibility/registry.json
integrations/antigravity/hooks/README.md
integrations/antigravity/plugin/README.md
integrations/antigravity/plugin/bridge-manifest.json
tooling/acceptance/implementation-traceability.json
tooling/evidence/p12-revalidation-implementation-repair.md

## Storage and exact blockers before P12 restart

| Drive | Free bytes |
|---|---:|
| C: | 2,857,119,744 |
| D: | 39,483,428,864 |

Remaining blockers are production updater signing/live endpoint verification,
the signed Antigravity bridge and installed runtime, the carried external
PostgreSQL/provider/cross-platform/distribution/usability/reboot/AI/browser
gates above, and the separately required P12 certification rerun. The repair
implementation and Windows-local regression are complete; P12 remains
REVALIDATION_REQUIRED until those blockers are separately addressed.

