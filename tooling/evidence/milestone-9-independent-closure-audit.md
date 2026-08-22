# Relintor P9 Independent Closure Audit

## Scope and disposition

This is the single current P9 source closure audit requested by the P9 audit
pack. Production P9 paths were inspected in `crates/relintor-execution`, the
desktop Tauri command boundary, and the P7/P8 integration points. Concrete
defects found during the review were repaired in this pass. No audit archive
was created, GitHub was not touched, `spec/locked` was not modified, and P10
was not started.

## Areas A–T

| Area | Status | Current evidence |
|---|---|---|
| A — J-01 checkpoint authority | PASS | Chained HMAC-authenticated records and index, sequence/parent binding, durable atomic writes, and fail-closed deletion/tamper tests passed. |
| B — checkpoint safe-boundary truth | DEFECT_FOUND_AND_REPAIRED | Resume now derives safety from checkpoint kind, authenticated run state, active attempts, and leases; pre-mutation checkpoints cannot authorize automatic resume. |
| C — J-02 Git/worktree | PASS | Git access remains read-only and optional; no reset, checkout, clean, push, or GitHub operation exists in the P9 path. |
| D — J-03 untracked/path safety | DEFECT_FOUND_AND_REPAIRED | Link/reparse entries now affect workspace fingerprints, snapshot reads are digest-consistent, secret/browser credential paths are omitted, and build-like user paths are no longer excluded by filename alone. |
| E — J-04 local database checkpoint | DEFECT_FOUND_AND_REPAIRED | SQLite checkpointing now requires the registered bounded adapter, enforces a real time deadline, rejects unsafe paths, validates backup artifacts, and rejects invalid restore files. |
| F — J-05/J-06 emergency stop | DEFECT_FOUND_AND_REPAIRED | Desktop mutation commands now share a Rust authority lock; durable recovery is preferred over a stale ledger; failed persistence remains an error and stopped-incomplete is never completion. |
| G — J-07 crash detection | DEFECT_FOUND_AND_REPAIRED | Session markers bind run, process ID, process-start identity where available, executable, and command; cross-run active markers do not create current-run crash evidence; crash records are authenticated. |
| H — J-08 process ownership | PASS | PID/image matches remain insufficient for termination; unknown, reused, inaccessible, and completed states fail closed and no PID-only kill path exists. |
| I — J-09 external edits | DEFECT_FOUND_AND_REPAIRED | Git HEAD/branch/index/worktree state is compared in addition to streamed content hashes; same-size and link changes force revalidation. |
| J — J-10 resume integrity | DEFECT_FOUND_AND_REPAIRED | Rust derives resume disposition; unsafe run states, task attempts, leases, authority changes, workspace changes, and Git changes do not become safe resume. |
| K — J-11 revalidation | DEFECT_FOUND_AND_REPAIRED | `BLOCKED_EXTERNAL` and authority mismatches now create durable P7/P8 revalidation records; P9 does not issue verification or completion authority. |
| L — J-12 compensation authority | DEFECT_FOUND_AND_REPAIRED | File compensation is bound to the trusted Rust handler, workspace-relative target, prior digest/content, post-action digest, and external-edit precondition. |
| M — compensation recoverability | DEFECT_FOUND_AND_REPAIRED | Compensation journals are authenticated and durable with PLANNED/STARTED/SUCCEEDED/FAILED state; an unresolved or completed attempt is not repeated blindly. |
| N — P7/P8 boundary | PASS | P9 only inspects execution/verification state and requests revalidation; it does not mint DONE, VERIFIED, VERIFIED_COMPLETE, evidence PASS, or certificates. |
| O — desktop authority | DEFECT_FOUND_AND_REPAIRED | Renderer commands provide only project identity/intent; Rust loads authority, derives disposition, serializes mutations, and persists state. |
| P — production/test separation | PASS | Test fixtures remain in test targets; production adapter absence remains an explicit external blocker and no synthetic success path was found. |
| Q — real persistence | DEFECT_FOUND_AND_REPAIRED | Recovery journals/checkpoints are store-bound and authenticated; load ordering now reconciles a newer durable recovery checkpoint instead of silently preferring stale ledger state. |
| R — sealed interruption scenarios | PENDING_EXTERNAL_ENVIRONMENT | Desktop/executor interruption, external edit, corruption, and recovery harnesses pass; real Antigravity interruption and a workstation reboot were not available/performed. |
| S — Windows symlink/reparse smoke | NOT_RUN | Link creation was unavailable without changing machine policy; the report preserves `P9_SYMLINK_REPARSE_CHECK=NOT_RUN`. Code-level link/reparse tests remain green where the environment permits them. |
| T — test quality | PASS | The real temporary filesystem, SQLite, Git, authenticated reload, and subprocess corpus exercises 41 P9 acceptance tests without DTO-only success assertions. |

## Defects repaired

| ID | Severity | Production consequence | Files changed | Test/evidence | Final disposition |
|---|---|---|---|---|---|
| P9-CLOSE-01 | High | A checkpoint from an unsafe or externally active boundary could have been labeled resumable. | `crates/relintor-execution/src/recovery.rs`, `crates/relintor-execution/src/lib.rs` | `before_mutation_checkpoint_is_not_an_automatic_resume_authority`; P9 corpus 41/41 | Repaired; disposition is Rust-derived and fail-closed. |
| P9-CLOSE-02 | High | Git path parsing could lose or corrupt changed paths, and Git metadata changes were not compared on resume. | `crates/relintor-execution/src/recovery.rs` | `git_snapshot_preserves_actual_changed_paths`; workspace external-change regression | Repaired; read-only Git state now participates in revalidation. |
| P9-CLOSE-03 | High | A newly introduced symlink/reparse entry could disappear from the workspace fingerprint. | `crates/relintor-execution/src/recovery.rs` | `symlink_addition_changes_workspace_fingerprint_when_supported`; existing escape checks | Repaired; link entries are fingerprinted without following them. |
| P9-CLOSE-04 | High | Same-run/session identity did not include process-start identity, and another run could be misclassified as current-run crash evidence. | `crates/relintor-execution/src/recovery.rs` | `active_session_from_another_run_does_not_affect_current_run`; crash-marker corpus | Repaired; markers and crash records are authenticated and run-bound. |
| P9-CLOSE-05 | High | SQLite backup timeout was declarative only, and arbitrary restore bytes could return success. | `crates/relintor-execution/Cargo.toml`, `crates/relintor-execution/src/recovery.rs` | `sqlite_restore_rejects_a_non_database_artifact`; real SQLite checkpoint test | Repaired; registered adapter, progress deadline, path checks, and SQLite integrity validation are enforced. |
| P9-CLOSE-06 | High | Caller-supplied compensation content/handler metadata could be detached from the registered target, and attempts were memory-only. | `crates/relintor-execution/src/recovery.rs` | `compensation_rejects_a_forged_prior_snapshot`; `compensation_journal_survives_reload_and_blocks_repeat_execution` | Repaired; trusted handler binding and authenticated durable journal prevent blind replay. |
| P9-CLOSE-07 | High | Concurrent desktop mutations and a crash between recovery checkpoint and ledger persistence could admit stale state. | `apps/desktop/src-tauri/src/lib.rs` | Full MSVC workspace regression; P9 interruption corpus | Repaired; mutation authority is serialized and recovery checkpoint state is reconciled with the ledger. |
| P9-CLOSE-08 | Medium | `BLOCKED_EXTERNAL` revalidation requests were not always materialized as durable handoff records. | `apps/desktop/src-tauri/src/lib.rs` | Full workspace/P8 regression and traceability gate | Repaired; blocked/uncertain states now remain pending P7/P8 authority. |

No unresolved critical or high P9 source defect remains from this A–T pass.

## External truth

- Real Antigravity crash/restart smoke: `PENDING_EXTERNAL_ENVIRONMENT`.
- Real workstation reboot smoke: `PENDING_EXTERNAL_ENVIRONMENT`.
- Windows symlink/reparse creation smoke: `NOT_RUN` because link creation was unavailable.
- Native MSVC Tauri rerun after this repair: `BLOCKED_LOCAL_STORAGE`; C: fell to 387,923,968 bytes free after the workspace build. The immediately preceding native no-bundle build passed before this audit repair and is preserved only as historical evidence.
- macOS/Linux hosted execution: not run in this local audit.

## Final audit result

`PASS — A–T reconciled; concrete defects repaired; no unresolved critical/high
P9 source defect.`

P10 remains **NOT STARTED**.
