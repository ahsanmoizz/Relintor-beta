# Relintor AI Handoff

## Read first

1. [`RELINTOR_REPOSITORY_VERIFICATION.md`](RELINTOR_REPOSITORY_VERIFICATION.md)
   — current evidence, contradictions, artifacts and next task.
2. [`RELINTOR_COMPLETE_TECHNICAL_ANATOMY.md`](RELINTOR_COMPLETE_TECHNICAL_ANATOMY.md)
   — architecture, authority, data, security and release boundaries.
3. [`RELINTOR_COMPLETE_USER_ADMIN_MANUAL.md`](RELINTOR_COMPLETE_USER_ADMIN_MANUAL.md)
   — supported user/admin journeys and truthful Beta messaging.
4. `README.md`, active manifests, source/configuration, `spec/locked/`, CI and
   the latest milestone evidence under `tooling/evidence/`.

`PROJECT_CONTEXT_TRANSFER.md` was requested but is missing from the repository
and attachment search. Treat any later-restored copy as historical input until
its claims are reconciled with the repository and executable evidence.

## Current lifecycle stage

Relintor is in Windows x64 closed-Beta release hardening after P12. The current
source contains an inert, non-null updater configuration that prevents the old
startup deserialization panic, but startup closure and website distribution
closure are not yet fully evidenced. Do not start another product milestone.

## Current objective

Close the current Windows Beta release chain truthfully: verify repaired startup,
run Rust gates under x64 MSVC, align the website `dist` download with the current
x64 installer, and preserve explicit external/cross-platform blockers.

## Source-of-truth order

1. Active repository source/configuration and executable artifacts.
2. `spec/locked/`, its manifest hash, manifests and lockfiles.
3. Executed tests, verification scripts, CI logs and captured runtime output.
4. Canonical docs and milestone evidence.
5. Historical transfer notes, old configs and renamed binaries.

## Important constraints

- Do not modify `spec/locked` or freeze-breaking lockfiles during verification.
- Do not expose secret values; environment-variable names may be documented.
- Do not add fake updater keys/endpoints or weaken signature verification.
- Missing updater production values must remain unavailable/fail-closed.
- Do not claim Google, PostgreSQL, AI provider, Antigravity, ARM64,
  macOS/Linux, signing, clean-machine or hosted-CI certification without direct
  evidence.
- Keep normal runtime machine-independent; development D: paths are not product
  configuration.
- Do not touch GitHub or publish artifacts unless explicitly requested.
- Keep website, desktop, cloud and AI-gateway changes scoped to the active task.
- Do not treat an old binary, empty error log or UI label as complete runtime
  evidence without an exit code and the matching source/artifact hash.

## Current next task

Add/run the updater configuration regression test, launch the repaired x64
executable with an explicit exit code, run fmt/clippy/workspace tests under the
pinned x64 MSVC toolchain, then rebuild and verify website `dist` against the
current installer SHA-256:

`95CD461A9D9EDF5598A1A406B257B373B0B24FA195F0A8046B27ED618EABC250`

Stop and report a precise blocker if the MSVC toolchain, clean-machine harness,
website deployment, or any required external service is unavailable.
