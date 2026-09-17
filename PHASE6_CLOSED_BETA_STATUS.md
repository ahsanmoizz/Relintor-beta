# Relintor Closed-Beta Hardening Status

Last updated: 2026-09-16 08:17:07 local time

## Sealed plan

1. Desktop smoothness + existing-project opening — COMPLETE
2. Storage/checkpoint/low-disk safety — COMPLETE
3. Authority/status consistency — COMPLETE
4. Windows packaging/installer — NEXT
5. Security/privacy — NOT STARTED
6. Diagnostics/supportability — NOT STARTED
7. Cross-project robustness — NOT STARTED
8. Crash/restart qualification — NOT STARTED
9. Reliability qualification — NOT STARTED
10. Closed-Beta freeze/release — NOT STARTED

No phase was added, removed, reordered, or expanded into new product work.

## Source lineage

Historical pre-hardening local HEAD:
`81605000624fa2b0476f310f46409cca15c21caf`

Phase 1 implementation snapshot:
`3898415b1a626ead6437a31eb17c2d4694ed69c2`

Phase 2 implementation snapshot:
`5d6d341ed3b835807016f4caaa96a5b45c28eba1`

Phase 1 qualified source hashes:

- `apps/desktop/src-tauri/src/lib.rs`
  `01F8A2BEF06992F7C6952410F0A5C7B83F0BB12D34B3F303B3DF1CC8B0C80B25`
- `apps/desktop/src/App.tsx`
  `A1E5F9632413B7C4B75B966EF1D71CA07D0EE1E75DA1730756383536E088D2F1`

Phase 2 qualified source hashes:

- `crates/relintor-execution/src/recovery.rs`
  `8EA558115FB6D2463B2E85FFC217298477CBB6ED9C0FA54163CEB2E8E8E0BBBE`
- `crates/relintor-execution/tests/p9_recovery_acceptance.rs`
  `4FB56AEECF08CF45FC6E4B9A71A3BC298F299F091AE052034D9983786B67741D`

## Phase 1 — Desktop smoothness + existing-project opening

Status: **COMPLETE**

Qualified behavior:

- 30/30 cold-launch/project-open qualification runs passed.
- Cold usable-window p95: **2215 ms** (gate <= 3000 ms).
- Existing-project open p95: **249 ms** (gate <= 2000 ms).
- Projects-list p95: **198 ms**.
- Lightweight persisted-status p95: **0.4458 ms** (gate <= 100 ms).
- Serious existing project open: **194 ms**, PASS.
- Repeated project switching/stale-state isolation: PASS.
- Frontend suite: **63/63 PASS**.
- Rust check/test-target compile and release Tauri build: PASS.

Implementation summary:

- Saved-project list/open paths return persisted state without synchronous whole-workspace reconciliation.
- Fresh reconciliation remains at the authority-review boundary.
- Duplicate saved-project opens are single-flight guarded.
- No UI redesign or governance-semantic change.

Evidence:
`RELINTOR_PHASE1_QUALIFICATION_V2_20260915-214058.zip`

## Phase 2 — Storage, checkpoint & low-disk safety

Status: **COMPLETE**

Measured baseline:

- Production Relintor AppData: **8,036,072,109 bytes (7.48 GB)**.
- Recovery/checkpoint candidates: **8,001,036,167 bytes (7.45 GB)** across 368 artifacts.
- SQLite DB: **4,042,752 bytes (3.86 MB)**.
- Exact duplicate-file waste: only **44,756 bytes**, proving duplicate deletion was not the root fix.
- SQLite baseline journal mode was `delete`; WAL was not assumed or forced.
- Baseline SQLite `quick_check` and `integrity_check`: PASS.

Root cause:

- Recovery checkpoints repeatedly embedded thousands of untracked workspace files as raw payload bytes.
- Serious checkpoint `untracked` payload was about 89 MB and `run_snapshot_json` about 19 MB; historical checkpoints reached about 261 MB.
- The historical recovery chain could not be safely pruned by arbitrary deletion because lineage validation depends on earlier sequence/digest authority.

Qualified implementation:

- Checkpoint workspace/untracked recovery capture is metadata-only instead of repeatedly embedding workspace file bytes.
- Latest full recovery payload remains intact.
- Immediately previous full recovery payload remains intact as the pre-write rollback boundary.
- Older superseded history is compacted to authenticated chain stubs, preserving lineage/provenance instead of deleting history.
- Existing temp-write, fsync, atomic replace and low-disk protections remain in place.
- Tampered compact history fails closed.
- Interrupted/mixed compaction still resolves to valid latest authority.

Validation:

- Fresh Cargo target used; stale compiled test reuse excluded.
- Recovery regression suite: **54/54 PASS**.
- Existing Phase-5 restart/low-disk suite: **29/29 PASS**.
- Fresh release Tauri build: PASS.
- Qualified release EXE SHA-256:
  `FE00F75502E1C21F052E4214F00D3788DBA89ACF3961B6EE5A88E469C3003574`
- 100 read/open/poll cycles with no new mission state: **0 bytes persistent growth** (gate <= 10 MB).
- Large valid checkpoint round-trip: PASS.
- Large test run snapshot: **20,971,520 bytes**.
- Large recovery tree: **22,032,877 bytes**.
- Checkpoint write samples recorded:
  `[881, 1329, 3214, 3483, 3467, 3462, 3484, 3488, 3522, 26305] ms`
- Checkpoint write p50: **3483 ms**.
- Checkpoint write p95/max in that isolated sample: **26305 ms**.
- Legacy all-full chain safely compacts on the next checkpoint: PASS.
- Low disk before write: PASS.
- Disk full during temp write model: PASS.
- Restart after failed write: PASS.
- Interrupted checkpoint commit: prior authority preserved, PASS.
- Interrupted cleanup/compaction: PASS.
- Compact-stub tampering fails closed: PASS.
- Previous valid authority remains readable: PASS.
- Deterministic reopen: PASS.
- SQLite `quick_check` + `integrity_check` passed after every injected failure.
- Production AppData bytes before/after final qualification were identical:
  **8,036,072,109 -> 8,036,072,109**.
- Production DB SHA-256 remained identical:
  `50051CE1034C2FEC79EDD329C1328A93B5AE5F937419F998134E1DA509B69535`.
- Phase 1 files remained bit-for-bit unchanged through Phase 2 qualification.
- Phase 3+ was not executed.

Principal evidence:

- `RELINTOR_PHASE2_APPLY_VALIDATE_V5_20260916-073421.zip`
  SHA-256 `C392C972E02D44D8CDD987A3AD8460A7E9EDFE0452F9D54855D7C2E55F1EA19B`
- `RELINTOR_PHASE2_FINAL_QUALIFICATION_20260916-080243.zip`
  SHA-256 `AA77C79AD5E9CE8A038CEFD29DADAE9A796FB579FDC9780E1A0DE79045741575`
- `RELINTOR_PHASE2_FINAL_RESUME_20260916-080917.zip`
  SHA-256 `B412E8874A77041AD187BE3229379BB3701ADDF0D3467D18F91596A92E007836`

## Protected-state conclusion

The Phase-2 qualification demonstrated bounded no-state-change growth, preservation of prior authority across write failures/interruption, authenticated historical lineage after compaction, fail-closed tamper behavior, and database integrity after injected failures. No protected HumanDecision/rejection/current-authority state was intentionally deleted or rewritten by compaction.

## Phase 3 — Authority & Status Consistency

Status: **COMPLETE**

Objective:

- Every user-visible Relintor surface now projects the same backend-authoritative truth for the current project / mission / revision.
- The known contradiction class where `VERIFIED COMPLETE` could coexist with a global `AUTHORITY NEEDS ATTENTION` presentation is closed without introducing a new authority model.

Qualified implementation:

- Frontend refresh/open projection returns backend workflow authority instead of preserving stale local authority.
- `verification_start` and `verification_status` remain tied to the same backend workflow-stage derivation.
- Final HumanDecision controls fail closed unless the backend is genuinely `WAITING_FOR_USER_DECISION`.
- Machine-verifiable missing/failed/blocked/correction-required states cannot expose final approval/rejection controls.
- Verified Complete presentation requires coherent backend `VERIFIEDCOMPLETE` authority plus a valid certificate.
- Generic system-health messaging no longer masquerades as mission authority.
- Home no longer emits the false `no missions` placeholder while authoritative mission state exists.
- Historical rejection/correction state is not promoted into approval.
- No new authority model, governance layer, Verified Complete definition, or frontend-only authority override was introduced.

Validation:

- Targeted authority-presentation tests: **41/41 PASS**.
- Full frontend tests: **80/80 PASS**.
- Frontend typecheck: PASS.
- Frontend production build: PASS.
- Existing Phase-5 restart/authority regression matrix: **29/29 PASS**.
- Rust desktop check: PASS.
- Desktop Rust test-target compile: PASS.
- Phase-1 and Phase-2 qualified files remained bit-for-bit unchanged.

Real persisted-project proof:

- Used a COPY of the historical serious Phase-5 persisted project/mission; the production persisted state was not modified.
- Persisted mission:
  `mission-takeover-project-takeover_719ad83a558ede1be5868c6d`
- Persisted project:
  `takeover-project-takeover_719ad83a558ede1be5868c6d`
- Revision: **1**
- SQLite `quick_check`: **ok**.
- SQLite `integrity_check`: **ok**.
- Repeated copied-DB reopen: **20/20 stable**.
- Repeated copied execution-ledger reads: **20/20 stable**.
- Verification artifacts: **56 files**.
- Evidence artifacts excluding certificate: **55 files**.
- Persisted certificate present with final state: **VERIFIED_COMPLETE**.
- Real persisted projection: **VERIFIED_COMPLETE**.
- Production persisted state before/after qualification: unchanged.

Sealed acceptance:

- Authority state matrix: PASS.
- Zero contradictory user-visible authority states: PASS.
- `verification_start` / `verification_status` shared authority: PASS.
- Final controls fail closed on machine blockers: PASS.
- Genuine HumanDecision is not inferred by frontend presentation: PASS.
- Historical rejection is not promoted to approval: PASS.
- Rejection → correction → reverification presentation transitions: PASS.
- Historical/completed attempt recovery immunity: PASS.
- Refresh/reopen causes no authority flip without an authoritative transition: PASS.
- Verified Complete requires coherent backend stage + certificate: PASS.
- Certificate-eligibility projection: PASS.
- Real persisted project copy proof: PASS.
- Phase 4 was not executed.

Principal evidence:

- `RELINTOR_PHASE3_APPLY_VALIDATE_20260916-084830.zip`
  SHA-256 `968612F7D2FEA94F7A951412BAEB481FE26791A86D523181703EFD4CE1AEF085`
- `RELINTOR_PHASE3_FINAL_QUALIFICATION_20260916-090408.zip`
  SHA-256 `AE414BA2BDD1CEF78604BBE71057D73142FBDA1EC09B1080CA53C0710BE83A1B`

## Next sealed phase

**Phase 4 — Windows Installer & Packaging Closure**

Not started in this snapshot.
