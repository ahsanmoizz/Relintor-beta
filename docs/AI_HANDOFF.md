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

Phase 1 remains `REAL MANUALLY VERIFIED`. Phase 2, Phase 3 and Phase 4 now have
`CI VERIFIED` evidence at commit
`9bd9fdce59ce20ce5014e885defeb23084f6ab9a`:

- Quality run `32657628058`: Windows, macOS and Ubuntu PASS.
- Windows Closure run `32657628151`: PASS.
- Secret Hygiene run `32657628054`: PASS.

This is CI closure only. The same canonical Windows mission still needs the
real continuity, restart/reload, bounded complex execution and completion-
certificate proof described by the project handoff. Phase 5 remains blocked;
do not start it.

## Six-phase tracker

1. Phase 1 — `REAL MANUALLY VERIFIED`
2. Phase 2 — `CI VERIFIED`; real continuity/restart proof pending
3. Phase 3 — `CI VERIFIED`; real complex/unattended execution proof pending
4. Phase 4 — `CI VERIFIED`; real evidence/correction/certificate/reload proof pending
5. Phase 5 — `BLOCKED`
6. Phase 6 — `BLOCKED`

These statuses are based on the concrete CI runs above. Do not promote any
phase to `REAL MANUALLY VERIFIED`, `BETA READY`, or `PRODUCTION READY` without
the corresponding runtime evidence.

## Current objective

Continue the same canonical Windows mission through Phase 2 continuity,
Phase 3 bounded complex execution, and Phase 4 evidence/correction/
certificate/reload verification. Preserve the existing mission and history;
do not start Phase 5.

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
- Keep GitHub changes limited to the requested `phase2-4-ci` branch / PR #1;
  do not publish artifacts or make unrelated repository changes.
- Keep website, desktop, cloud and AI-gateway changes scoped to the active task.
- Do not treat an old binary, empty error log or UI label as complete runtime
  evidence without an exit code and the matching source/artifact hash.

## Current next task

Use the existing canonical Windows workspace/mission and prove, through the
normal UI, restart/reload continuity and recovery authority; then run the
bounded complex/unattended execution proof and the P8 evidence, correction,
certificate and reload proof. Only after those runtime gates pass may the
tracker move beyond CI closure or Phase 5 unblock.
