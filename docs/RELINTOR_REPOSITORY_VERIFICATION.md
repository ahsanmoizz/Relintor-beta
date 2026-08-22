# Relintor Repository Verification

## Current audit record

**Snapshot:** 2026-08-18, repository `D:\Relintor`.

**Purpose:** permanent, repository-authoritative reconciliation of the current
Relintor implementation, executable artifacts, validation evidence, and closed
Beta readiness. This document does not authorize product implementation.

### Evidence vocabulary

| Label | Meaning |
|---|---|
| `VERIFIED` | Direct source/config inspection or an executed test/gate supports the claim. |
| `IMPLEMENTED / NOT LIVE VERIFIED` | Deterministic source/tests exist, but the real external or clean-machine path did not execute. |
| `PARTIALLY IMPLEMENTED` | A boundary or foundation exists, but the supported product path is incomplete. |
| `UNVERIFIED` | The repository or current executable evidence cannot prove the claim. |
| `FUTURE` | Intentionally described work that is not implemented. |
| `BLOCKED_ENVIRONMENT` | The check could not execute because the current environment lacks a required dependency/toolchain/service. |

## Transfer-document reconciliation

`PROJECT_CONTEXT_TRANSFER.md` was requested as the first input but was not found
at the repository root, under `D:\CodexHome\attachments`, or by repository file
search. Its historical/product claims and its final documentation-package list
are therefore `UNVERIFIED`. No claim from that missing document is promoted over
current source, executable artifacts, tests, CI or the locked specification.

Source-of-truth order for future work:

1. Active repository source and configuration.
2. `spec/locked/`, `spec/LOCKED_MANIFEST.sha256`, manifests and lockfiles.
3. Executed tests, verification scripts, CI logs and executable artifacts.
4. Canonical docs and milestone evidence.
5. Historical transfer notes, audit packs and old binaries, clearly labeled as
   historical or unverified.

## Repository shape and exact counts

| Area | Current fact | Evidence |
|---|---:|---|
| Applications | 3 | `apps/desktop`, `apps/admin`, `apps/website`. |
| Rust workspace members | 12 | Root `Cargo.toml`; Tauri crate, 9 reusable crates, 2 services. |
| Reusable Rust crates | 9 | Directories under `crates/`. |
| Services | 2 | `services/cloud-api`, `services/ai-gateway`. |
| Workspace package directories | 3 | `packages/config`, `packages/contracts`, `packages/ui`. |
| Rust source files | 41 | Current `.rs` files under `crates/`, `services/` and `apps/desktop/src-tauri`, excluding generated/build trees. |
| Application TypeScript/TSX files | 16 | Current `.ts`/`.tsx` files under `apps/`, excluding dependency/build trees. |
| Python tooling scripts | 14 | Python files under `tooling/`. |
| Acceptance Python scripts | 13 | Python files under `tooling/acceptance`. |
| PostgreSQL/SQLite migration files | 10 | `db/migrations/001` through `010`. |
| Locked specification files | 18 | Current spec verifier result. |
| Locked feature records | 144 | `files=18`, `manifest_entries=17`, `features=144`, `unique_ids=144`. |
| Rust integration-test files | 19 | `.rs` files in crate `tests` directories. |
| Frontend test files | 3 | Desktop, admin and website `App.test.tsx`. |

Workspace/toolchain facts:

- Rust workspace edition is 2021 with `rust-version = 1.96`.
- `rust-toolchain.toml` requests channel `1.96.0`, minimal profile, rustfmt and
  clippy.
- Root package manager is pnpm `11.17.0` and `pnpm-lock.yaml` is present.
- Tauri major version is 2; the desktop crate depends on
  `tauri-plugin-updater = "2"`.
- `spec/locked` and the lockfiles were not changed during this documentation
  audit.

## Architecture verification

### Desktop

`apps/desktop` is a React/TypeScript renderer over the native
`apps/desktop/src-tauri` Rust command boundary. Current renderer destinations
are `home`, `projects`, `activity` and `account`. The native layer owns local
SQLite, health, Google browser authentication, mission authority, execution,
verification, diagnostics and distribution commands.

The renderer can present state and invoke commands; it does not mint evidence,
entitlements, certificates or completion authority. Browser-preview paths are
explicitly separate from native authority.

### Public website and admin portal

- `apps/website` is a public React presentation/download surface. It is not
  execution or verification authority.
- The website source has a primary `Download for Windows` link at
  `/downloads/Relintor_0.1.0_x64-setup.exe` and labels it `Windows x64 · Closed
  Beta`.
- `apps/admin` is a React portal-style cloud/admin surface. Server role, tenant,
  MFA and entitlement checks remain authoritative; UI controls do not grant
  access.

### Local/cloud boundary

Local SQLite and the Rust authority own project/mission/revision/execution and
evidence state. Cloud PostgreSQL owns users, organizations, memberships,
devices, sessions, plans, entitlements, usage, grants, subscriptions, admin
identity/MFA/audit records and Google external identities.

`services/cloud-api` contains a PostgreSQL store and a deterministic in-memory
store for tests. The in-memory store does not prove live PostgreSQL behavior.

### Antigravity

The compatibility registry recognizes CLI names including `agy`/`agy.exe` and
version prefix `1.`. The source contains discovery, protocol and process-safety
boundaries. The bridge manifest still has externally required signing/digest
fields and no production signed bridge package is present. Real Antigravity
execution and interruption/reboot evidence are `PENDING_EXTERNAL_ENVIRONMENT`.

### Cloud API and authentication

Source-visible route families include:

- `/healthz`, `/readyz`;
- `/v1/auth/dev/sign-in`, `/v1/auth/google/exchange`, `/v1/auth/refresh`,
  `/v1/auth/sign-out`;
- `/v1/account`, `/v1/devices/register`, `/v1/entitlements`;
- `/v1/organizations`, `/v1/organizations/members`,
  `/v1/organizations/policy`;
- `/v1/billing/status`;
- `/v1/admin/audit`, `/v1/admin/grants/*`, `/v1/admin/mfa/*`;
- `/v1/ai/complete` and `/v1/service/status`.

Google OIDC validation is source-backed: issuer/audience/expiry/subject,
verified-email and JWKS signature checks are implemented. Deterministic tests
exist; a real Google login remains `IMPLEMENTED / NOT LIVE VERIFIED`.

The AI Gateway has explicit provider selection, server-side credential names,
bounded responses, request deadlines, usage accounting and fail-closed mock
selection. DeepSeek is a server-side provider boundary; a live provider call is
not proven by local deterministic tests.

## Current updater/startup evidence

### Historical failure

`relintor-launch-err.txt` records the old installed binary failing with exit code
101 at `lib.rs:2612` because `plugins.updater` deserialized as `null` while
`tauri_plugin_updater` was registered.

### Current source/config state

The active `apps/desktop/src-tauri/tauri.conf.json` now contains:

```json
"plugins": {
  "updater": {
    "pubkey": "",
    "endpoints": []
  }
}
```

This is a valid inert plugin configuration, not a fake public key or endpoint.
The native updater builder still requires `RELINTOR_UPDATE_PUBLIC_KEY` and
`RELINTOR_UPDATE_ENDPOINT`, rejects empty values, rejects non-HTTPS production
endpoints, and only permits loopback HTTP for explicit local tests. Signature
verification remains delegated to the Tauri updater boundary.

`apps/desktop/src-tauri/tauri.conf.before-updater-fix.json` is a historical
pre-fix snapshot and must not be treated as the active configuration. No
updater-specific regression test was found in the current native test module;
that is a concrete remaining engineering task.

### Current executable artifacts

| Artifact | Size | Last write | SHA-256 | Interpretation |
|---|---:|---|---|---|
| `target/release/bundle/nsis/Relintor_0.1.0_x64-setup.exe` | 6,043,275 bytes | `2026-08-18T22:55:58.6358574+05:00` | `95CD461A9D9EDF5598A1A406B257B373B0B24FA195F0A8046B27ED618EABC250` | Newer x64 installer artifact. |
| `relintor-desktop.exe` | 21,750,784 bytes | `2026-08-18T22:55:40+05:00` | `A21DEFF2360FEEBD182C93E9D7B1EA729DBCFBDA24E28136044A4A6C5BB24AB9` | Newer native executable artifact. |
| `relintor-desktop.OLD-BROKEN.exe` | 21,733,888 bytes | `2026-08-18T17:46:00+05:00` | `C9279ACCA3DCE566DC48CA594427239C8E94994EB0D3007EF34A35C678DEC2DE` | Historical pre-fix binary. |

`repaired-launch-err.txt` and `repaired-launch-out.txt` are empty, but no
explicit repaired-process exit code is recorded. Therefore startup repair is
`IMPLEMENTED / EXECUTABLE EVIDENCE INCOMPLETE`, not a final clean-machine PASS.

## Website distribution contradiction

The website source and public asset currently point at the newer installer:

- `apps/website/public/downloads/Relintor_0.1.0_x64-setup.exe` — 6,043,275 bytes,
  SHA-256 `95CD461A9D9EDF5598A1A406B257B373B0B24FA195F0A8046B27ED618EABC250`.

The generated website distribution is stale:

- `apps/website/dist/downloads/Relintor_0.1.0_x64-setup.exe` — 6,041,045 bytes,
  SHA-256 `9644DEDA0C114A684E9A2B45A339252520FB433C74802B03BA20583FDD7D7199`.

This is a real deployment contradiction. The website must be rebuilt and its
served download hash checked before the current x64 installer can be called the
website-delivered Beta artifact.

## Current verification results

These gates were executed during this documentation audit:

| Command | Result | Evidence |
|---|---|---|
| `python tooling/spec/verify_spec.py` | `PASS` | Exit 0; 18 files, 17 manifest entries, 144 features, 144 unique IDs; manifest SHA-256 `b0a05c46b28d2e9dfe27233394ffd308e2f75302c57ce4b619bc9a6198d2be78`. |
| `python tooling/acceptance/verify_traceability.py` | `PASS` | Exit 0; 144 records, 144 unique IDs. |
| `python tooling/acceptance/secret_scan.py` | `PASS` | Exit 0; no provider/private-key/connection-secret patterns. |
| `pnpm --dir apps/desktop typecheck` | `PASS` | Exit 0. |
| `pnpm --dir apps/desktop lint` | `PASS` | Exit 0. |
| `pnpm --dir apps/desktop test` | `PASS` | Exit 0; 10 tests passed. React `act(...)` warnings were emitted but did not fail the suite. |
| `pnpm --dir apps/website typecheck` | `PASS` | Exit 0. |
| `pnpm --dir apps/website lint` | `PASS` | Exit 0. |
| `pnpm --dir apps/website test` | `PASS` | Exit 0; 2 tests passed. |
| `cargo fmt --all -- --check` | `BLOCKED_ENVIRONMENT` | The current runner has no installed Rust toolchains and attempted to sync GNU 1.96; the sync was stopped. No current Rust fmt/clippy/test result was produced in this audit. |

Historical milestone reports record earlier Rust/workspace/native/gitleaks
passes, but those are historical evidence and are not silently restated as a
fresh current-tree Rust result here.

## CI and release state

`.github/workflows/quality.yml` defines Windows, macOS and Ubuntu quality jobs
with frozen pnpm install, locked spec/traceability/secret gates, Rust fmt,
clippy, workspace tests, desktop frontend gates and a native Tauri no-bundle
compile. `.github/workflows/secret-scan.yml` runs Gitleaks on Ubuntu. Hosted
runner execution is not proven by the presence of workflow files alone.

Packaging directories for Linux, macOS and signing contain placeholders rather
than completed certified release artifacts. Windows x64 artifacts exist locally;
signing, clean-machine install/upgrade/uninstall, cross-platform, ARM64, live
PostgreSQL, live Google, live provider, live Antigravity and real updater
endpoint/key gates remain unverified or externally pending.

## Current beta disposition

`IMPLEMENTED / WINDOWS X64 ARTIFACT PRESENT / BETA DEPLOYMENT NOT CLOSED`.

The repository has a newer x64 executable/installer and the startup null-config
root cause is corrected in active configuration. The following prevent a final
closed-beta readiness claim:

1. Direct repaired-executable launch needs an explicit exit code and captured
   safe stdout/stderr.
2. A native regression test should prove the active updater configuration is an
   object with empty provisioning values and that missing production values stay
   unavailable.
3. Rust gates must run under the pinned MSVC toolchain, not the absent GNU
   toolchain attempted by the current runner.
4. Website `dist` must be rebuilt/redeployed so its served installer matches the
   current public asset and installer hash.
5. Clean-machine, signing, external-service and cross-platform gates remain
   separate requirements.

## Exact next engineering task

Close the Windows Beta release chain: add and run the updater configuration
regression test, launch the repaired x64 executable under a controlled harness
and record its exit code, run the Rust gates under the pinned x64 MSVC toolchain,
then rebuild/verify the website distribution so the served installer hash is
`95CD461A9D9EDF5598A1A406B257B373B0B24FA195F0A8046B27ED618EABC250`.

Do not start another product milestone until that chain is either evidenced or
truthfully marked blocked.
