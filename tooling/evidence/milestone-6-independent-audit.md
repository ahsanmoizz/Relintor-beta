# Relintor Milestone 6 — Independent Source Audit

## Verdict

`MILESTONE_6_REPAIR_REQUIRED`

The Windows-local verification report is internally consistent, but the independent
source audit found authority-breaking gaps that the current 13-test P6 acceptance
suite does not cover.

Do not start P7 until these findings are repaired and the independent P6 source
audit target passes.

## Critical findings

### P6-IA-01 — P4/P5 project truth is not imported into the sealed requirement graph

`AuthorityEngine::build_draft` accepts only project IDs/revisions/fingerprint,
registry, applicability context, scope, and decisions. It does not accept or merge
the actual P4 blueprint requirements, user/document requirements, takeover
capabilities/findings, ADRs, risks, or NFRs.

`architecture_decisions` is initialized to an empty vector.

The generated requirement graph therefore consists only of standards-derived
requirements. Relintor can seal a standards-only mission while omitting the
product the user actually asked to build.

Repair requirement:
- introduce a typed project-authority input/requirement-seed boundary;
- import P4/P5 requirements with real source provenance;
- import relevant ADRs/decisions/NFRs;
- merge project requirements with standards-derived requirements;
- prove every imported requirement is accounted for before sealing.

### P6-IA-02 — Missing facts silently become `NOT_APPLICABLE`

Primitive applicability predicates return `False` when a fact is missing. The
`Unknown` state is therefore effectively unreachable for ordinary missing facts,
and `NeedsDecision` is never produced by the current evaluator.

An empty/underspecified project can consequently turn almost every standards rule
into N/A instead of blocking for missing information.

Repair requirement:
- distinguish explicit false from missing/unknown;
- missing authority-bearing facts must produce `BLOCKED_BY_UNKNOWN` or
  `NEEDS_DECISION` according to policy;
- an underspecified mission must not seal by converting unknowns into N/A.

### P6-IA-03 — An applicable critical rule can disappear if its task/link is removed too

The existing acceptance test removes a requirement but deliberately leaves a
dangling task link, so pre-seal fails because of graph corruption.

`preseal` iterates the requirements that still exist; it does not independently
walk every `APPLICABLE` ledger rule and require its mapped requirement to still
exist.

If the requirement and its matching task/link are removed together, the critical
rule can evade the current accounting check.

Repair requirement:
- pre-seal must account from applicability ledger -> rule -> requirement;
- every applicable rule must have exactly one valid mapped requirement;
- applicable critical rules must fail closed if that requirement vanishes.

### P6-IA-04 — Applicability ledger can be forged after evaluation

`MissionDraft` exposes mutable applicability results and does not retain/bind the
authoritative applicability context in a way that pre-seal recomputes.

A caller can mutate an applicable rule to N/A, remove the requirement/task, and
present a self-consistent but false draft.

Repair requirement:
- store a canonical applicability context/fingerprint in the draft/contract;
- pre-seal deterministically re-evaluates the signed rule predicates;
- reject ledger/context mismatches.

### P6-IA-05 — `NON_WAIVABLE` critical rules can be deferred

`validate_decisions` blocks AI defer, and restricts N/A/explicit-exception policy,
but `DecisionKind::Defer` is accepted for any human/user decision regardless of
`ExceptionPolicy`.

That permits a human/user defer record to put a non-waivable critical requirement
into `DEFERRED_BY_EXPLICIT_DECISION` and still seal.

Repair requirement:
- enforce exception/defer policy for every decision kind;
- a `NON_WAIVABLE` rule cannot be deferred, waived, or marked N/A.

### P6-IA-06 — Task coverage can be spoofed because link and task declarations are not cross-checked

`TaskGraph::validate` validates `Task.requirement_ids` and separately validates
`TaskRequirementLink`, but it does not require a link to agree with the
corresponding task's declared requirement IDs.

Coverage is then computed from links.

Swapping links between valid tasks can preserve apparent coverage while the tasks'
own declared requirement ownership says something else.

Repair requirement:
- task links and `Task.requirement_ids` must be exactly consistent;
- dependency IDs carried inside tasks must also agree with the authoritative task
  dependency graph;
- duplicate links must be rejected.

### P6-IA-07 — Registry digest is not fully order-independent

Registry canonicalization sorts packs/rules, but `pack_digests` are calculated
from the pack's current serialized rule order before registry canonicalization.

Reordering semantically identical rules and refreshing metadata therefore changes
the pack digest and can change the registry digest.

Repair requirement:
- create one canonical pack serializer/digest function;
- pack rule ordering must not affect pack or registry digest;
- add order-independence tests for both pack and registry digests.

### P6-IA-08 — Seal metadata/manifest is not cryptographically bound or fully validated

`validate_seal` checks the contract hash and only registry ID/version/digest from
the manifest.

It does not bind/validate:
- manifest rule IDs;
- manifest rule digests;
- manifest pack versions;
- seal mission ID;
- seal revision;
- outer revision;
- duplicated scope fingerprint/state metadata.

Those fields can be changed while `validate_seal` still reports valid.

Repair requirement:
- cryptographically bind the complete authority-bearing seal manifest;
- validate mission/revision/scope consistency;
- reject any manifest/identity tamper.

### P6-IA-09 — Authority persistence cannot store stable IDs across missions/revisions

Migration 006 uses global primary keys:

- `authority_requirements(requirement_id PRIMARY KEY)`
- `authority_tasks(task_id PRIMARY KEY)`
- `authority_decisions(decision_id PRIMARY KEY)`

Requirement/task IDs are intentionally stable, so the same standards-derived IDs
recur in later revisions and other missions.

`persist_authority` uses `INSERT OR IGNORE`, causing later rows to be silently
discarded.

Repair requirement:
- use mission/revision-scoped composite authority keys;
- because migration 006 has already been applied in development, repair with a
  new additive migration (for example 007), not by rewriting migration 006;
- prove two missions and two revisions preserve their normalized rows.

### P6-IA-10 — Persistence/load do not enforce seal integrity

`persist_authority` validates registry schema but does not validate the supplied
mission revision's contract hash/seal binding before persistence.

`load_mission_revision` deserializes DB JSON and returns it without validating the
stored contract against the stored seal.

A tampered/forged revision can therefore cross the persistence boundary.

Repair requirement:
- persistence rejects a revision whose canonical contract hash, seal identity,
  manifest, registry binding, or scope binding is inconsistent;
- load either validates before return or exposes only a validated-load authority
  API to callers;
- same mission/revision with different content must fail, not be silently ignored.

### P6-IA-11 — The actual desktop Seal & Build authority path is not implemented

The desktop currently exposes `authority_preview` only.

The UI's Seal & Build button is permanently disabled and there is no Tauri
command that:
- loads/verifies a trusted signed registry;
- constructs a project-bound draft;
- runs pre-seal;
- seals;
- persists the immutable revision;
- returns the `READY_FOR_EXECUTION` handoff.

The builtin registry is unsigned and no production runtime trust anchor is wired.

This means the P6 library can seal test fixtures, but the product cannot yet
perform the sealed P6 action.

Repair requirement:
- wire a Rust-owned Seal & Build command;
- use a real bundled/installed signed registry plus trusted public key(s);
- never ship a private standards signing key;
- persist the sealed revision;
- return only a P7-ready handoff, never start P7.

### P6-IA-12 — Requirement `sealed_hash` is never populated

The requirement object defines `sealed_hash`, but generated requirements set it to
`None` and sealing never populates it.

Repair requirement:
- define deterministic per-requirement canonical hashing;
- populate/validate the per-requirement sealed hash as part of sealing;
- exclude the hash field itself from its own digest input.

## Independent repair gate

The included `independent_p6_source_audit.rs` adds adversarial tests for the most
important machine-testable findings.

It intentionally fails against the audited source.

After repairs, run:

`cargo test -p relintor-standards --test independent_p6_source_audit --locked -- --nocapture`

Then rerun:
- dedicated P6 acceptance;
- full workspace Rust tests;
- Python spec/traceability/secret gates;
- desktop typecheck/lint/test/build;
- native Tauri build;
- Gitleaks 13 scopes;
- final renderer/provider/signing-secret scan.

Do not start P7 until the independent audit is green and a repaired source package
has been reviewed.
