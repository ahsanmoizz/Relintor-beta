# 09 — Implementation Phases

The phases are sequential authority gates, not calendar promises. Later phases may begin exploratory work, but a phase cannot be certified complete until its gate passes.

## P0 — Truth Baseline & Repository Reconciliation
Build:
- rotate/revoke any exposed credentials;
- snapshot current Onus repository;
- classify product vs generated/legacy code;
- resolve duplicate console/source-of-truth;
- create clean release branch;
- map reusable Rust primitives.

Verify:
- clean checkout reproduces Rust tests/build;
- current failures documented;
- generated/dependency trees ignored;
- no secret scanning findings;
- migration inventory signed off.

## P1 — Cross-platform Desktop Shell
Build:
- Tauri + React shell;
- theme;
- Home/Projects/Activity/Account;
- local encrypted DB;
- keychain;
- update framework;
- health checks.

Verify on **Windows + macOS + Linux**:
- install/uninstall;
- launch;
- DB migration;
- keychain;
- theme/accessibility;
- update signature rejection test;
- no terminal required.

## P2 — Accounts, Entitlements & AI Gateway Foundation
Build:
- account service;
- device auth;
- signed entitlements;
- offline grace;
- AI gateway/provider abstraction;
- usage accounting.

Verify:
- no provider key in client;
- tampered entitlement rejected;
- offline grace works;
- founder free entitlement works;
- AI budget counted;
- provider outage fails cleanly.

## P3 — Antigravity Bridge
Build:
- detection;
- plugin;
- hooks;
- headless runner;
- permissions;
- structured events;
- compatibility suite.

Verify per OS:
- safe tool allowed automatically;
- denied tool cannot execute;
- changed payload invalidates grant;
- exit/result captured;
- Stop-hook limitation handled by scheduler;
- incomplete mission can continue in new turn.

## P4 — New Project Investigator
Build:
- document intake;
- intent model;
- questions;
- blueprint;
- ADR/risk/assumption records.

Verify with a diverse project corpus:
- important ambiguity detected;
- irrelevant questions suppressed;
- “not sure” yields recommendation;
- contradictory docs flagged;
- blueprint reproducible.

## P5 — Existing Project Takeover
Build:
- repository/routing/schema/test/runtime discovery;
- promise-vs-reality reconciliation;
- keep/repair/remove/rebuild report.

Verify against seeded repos containing:
- dead code;
- stale README;
- hidden routes;
- failing tests;
- duplicate implementations;
- broken builds;
- missing migrations.

## P6 — Standards + Requirement Graph + Sealing
Build:
- signed registry;
- 18 domain packs;
- applicability engine;
- requirement/evidence graph;
- task graph;
- seal hash.

Verify:
- irrelevant standards become N/A, not requirements;
- applicable critical rule cannot vanish;
- changed sealed requirement forces revalidation;
- every requirement has acceptance/evidence policy.

## P7 — Execution Scheduler & Watchdog
Build:
- task packets;
- leases;
- retries;
- loop detector;
- no-progress governor;
- cost/usage;
- clean-context continuation.

Verify adversarial runs:
- repeated failing command;
- edit/revert loop;
- early agent stop;
- task drift;
- excessive tool calls;
- external modification during run.

## P8 — Verification & Completion Authority
Build:
- evidence collectors;
- deterministic gates;
- browser/runtime verification;
- independent AI verifier;
- evidence invalidation;
- completion certificate.

Verify:
- agent claims “done” with missing test → rejected;
- deleted test → detected;
- stale evidence → rejected;
- wrong commit evidence → rejected;
- deterministic failure cannot be overridden by AI;
- all requirements accounted before `VERIFIED_COMPLETE`.

## P9 — Recovery & Resume
Build:
- checkpoints;
- crash recovery;
- emergency stop;
- orphan reconciliation;
- resume/revalidation;
- rollback registry.

Verify:
- kill desktop mid-edit;
- kill Antigravity;
- reboot between phases;
- modify files externally;
- corrupt local execution record;
- resume from last safe checkpoint without fake success.

## P10 — Teams, Billing & Founder Admin
Build:
- orgs/roles;
- team policy;
- subscriptions;
- complimentary grants;
- founder/admin portal;
- audit log.

Verify:
- founder account free/unlimited entitlement;
- free company grant;
- expired trial behavior;
- seat enforcement;
- admin MFA;
- unauthorized admin API access denied;
- all grant changes audited.

## P11 — UX Completion & Distribution
Build:
- final cockpit;
- empty/error/loading states;
- onboarding;
- accessibility;
- performance;
- website/hero;
- installers;
- support diagnostics;
- privacy controls.

Verify with non-expert usability sessions:
- user creates project without documentation;
- no shell;
- understands why mission is blocked;
- can stop/resume;
- can explain why completion is verified.

## P12 — Adversarial Release Certification
No new feature work.

Run:
- full cross-platform matrix;
- full 144-feature manifest;
- security review;
- failure injection;
- corrupted evidence tests;
- AI false-claim corpus;
- standards applicability corpus;
- billing/entitlement abuse tests;
- updater rollback;
- backup/export restore.

Ship only if the release gate in `10_VERIFICATION_AND_RELEASE_GATES.md` passes.
