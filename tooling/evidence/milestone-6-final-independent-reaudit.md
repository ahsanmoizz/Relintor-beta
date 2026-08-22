# Relintor P6 — Final Independent Re-audit

## Verdict

`MILESTONE_6_REPAIR_REQUIRED`

The first 12 audit findings are materially improved, and the existing
`independent_p6_source_audit` target now passes. The final repaired-source review
found additional product-path authority gaps that are not covered by the 10-test
audit target.

Do not start P7 until these are repaired.

## P6-FR-01 — Mixed known + unknown applicability can still seal

`AuthorityEngine::preseal` rejects unresolved `BLOCKED_BY_UNKNOWN` /
`NEEDS_DECISION` outcomes only when there are zero applicable rules.

A real project normally has at least one known applicable surface. That means
other unresolved domain facts can remain unknown and the mission can still seal.

The desktop production context always supplies `desktop=true` and `platform`,
while many other domain facts are omitted, so this is not hypothetical.

Repair:
- preseal must reject every unresolved authority-bearing applicability outcome,
  not only the fully-unknown-project case;
- or every domain fact must be explicitly resolved true/false before sealing.
  Unknown must never silently pass.

## P6-FR-02 — Desktop review and desktop seal are different authority contexts

`authority_preview` evaluates renderer-selected fact strings.

`seal_project_mission` ignores that reviewed fact set and separately derives facts
from persisted blueprint keyword matching.

The user can therefore review one standards/requirements set while Rust seals a
different one.

Additionally, the preview exposes only a subset of the 18 domain inputs.

Repair:
- build one Rust-owned project authority review from the actual persisted project;
- any user applicability decisions must be persisted/typed and included in the
  authority digest;
- return a draft/review digest;
- Seal & Build must recompute/validate that same reviewed authority before
  sealing.

## P6-FR-03 — Seal & Build is still unreachable from the GUI

`authority_preview` always returns two hard-coded blockers.

The button is disabled whenever `preview.blockers.length > 0`.

Therefore the Seal & Build button remains disabled for every preview even though
a backend command now exists.

Repair:
- blockers must be deterministic real preseal blockers;
- informational text such as "P7 not started" is not a sealing blocker;
- enable Seal & Build when the exact Rust-owned reviewed draft has zero blockers.

## P6-FR-04 — Mission revision lookup uses the wrong identity

`seal_project_mission` queries:

`mission_revisions WHERE mission_id = ?1`

with `project_id`, then constructs the real mission ID as
`mission-{project_id}`.

The lookup therefore does not find prior mission revisions and repeatedly
attempts revision 1.

Repair:
- construct the canonical mission ID first;
- query MAX(revision) using that exact mission ID;
- test seal revision 1, then reseal/revalidate to revision 2;
- preserve immutable revision 1.

## P6-FR-05 — Existing-project takeover is not scoped/bound to a Relintor project

The P5 schema has no project binding on `project_takeovers`.

P6 currently selects the latest takeover globally:

`SELECT id, fingerprint FROM project_takeovers ORDER BY created_at DESC LIMIT 1`

This can import findings from an unrelated repository.

The GUI also passes Seal & Build only
`investigation?.investigation.project_id`, so a takeover-only flow has no project
ID and cannot seal.

Repair with an additive schema change (do not rewrite older migrations):
- create an explicit project <-> takeover binding;
- select takeover authority only for the current project;
- make takeover-only mode produce/use a real Relintor project identity;
- prove project A can never import project B takeover findings.

## P6-FR-06 — SEO false is treated as applicable

The SEO pack uses `Exists { field: "seo_relevance" }`.

An explicit `seo_relevance=false` still satisfies `Exists`, making SEO rules
applicable.

Repair:
- use an explicit boolean truth predicate for SEO relevance;
- explicit false => NOT_APPLICABLE with reason;
- missing => unresolved/unknown until decided.

## P6-FR-07 — Seal state metadata can be changed without invalidating the seal

`seal_integrity_reasons` checks the contract/manifest/identity fields but does
not validate `MissionSeal.state`.

Changing `SEALED` to `EXECUTING` can still return a valid seal.

Repair:
- P6 persisted seal state must be exactly the P6 sealed state;
- P7 execution state must live in P7 execution records, not by mutating the
  immutable P6 seal;
- tampering the P6 seal state must invalidate authority.

## P6-FR-08 — Imported product requirements receive human-only proof by default

The desktop `project_seed` gives every imported user/blueprint/NFR/takeover
requirement:
- `HUMAN_DECISION`
- `HUMAN_ASSERTED`
- `machine_checkable=false`

That is acceptable for a genuine decision requirement, but not as the universal
proof policy for functional, security, migration, performance, or remediation
requirements.

P6 defines what evidence P8 will later require. A functional requirement should
not become verifiable solely because a person asserts that it is complete.

Repair:
- derive evidence obligations by requirement/NFR/finding type;
- use deterministic/runtime evidence for implementable behavior;
- reserve human decision evidence for actual decisions/approvals;
- keep P8 as collector/verifier owner.

## Closure

Run:
- `independent_p6_final_reaudit`
- existing `independent_p6_source_audit`
- existing P6 acceptance
- workspace regression
- desktop/native/security gates

Then provide a new final source ZIP before P7.
