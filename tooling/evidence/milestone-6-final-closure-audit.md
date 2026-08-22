# Relintor P6 — Final Closure Source Audit

## Verdict

`MILESTONE_6_CLOSURE_REPAIR_REQUIRED`

The prior P6 independent gates are green and the repaired source is materially
stronger. A final production-path inspection found four narrow gaps that must be
closed before P7 consumes the sealed task graph.

Do not reopen the whole phase. Repair only P6-FC-01 through P6-FC-04.

## P6-FC-01 — The production review UI cannot represent the full applicability model

Rust defines 17 target-project applicability facts:

- web
- backend
- database
- authentication
- ui_surface
- seo_relevance
- performance
- deployment
- observability
- privacy
- payments
- ai
- blockchain
- mobile
- desktop
- data_engineering
- integrations

The current GUI exposes only 8 of them. Missing GUI facts are:
AI, blockchain, data engineering, deployment, integrations, mobile,
observability, payments, and privacy.

`apply_reviewed_facts` then writes every fact to true/false based only on the
renderer list, so those hidden facts are forced false.

The UI also defaults `desktop` to true for every project, conflating the
Relintor host application with the target project.

This allows an AI, payments, blockchain, mobile, privacy-sensitive, integration,
or operational project to seal with its relevant standards marked N/A.

Repair:
- the target-project review must represent every P6 applicability fact;
- do not default target `desktop=true` merely because Relintor is a desktop app;
- user-reviewed false must be explicit, not the consequence of a field being
  impossible to select;
- preserve unknown when neither P4/P5 evidence nor an explicit user decision
  resolves a fact;
- normal GUI users must be able to resolve every remaining unknown without
  editing JSON or invoking Tauri manually.

A grouped Yes / No / Not sure review is preferred. Confident P4/P5-derived facts
may be prefilled, but unresolved facts must not silently become false.

## P6-FC-02 — Seal is not bound to the exact review displayed by the caller

`AuthorityPreview` returns `review_digest`, but the frontend calls
`sealProjectMission(projectId)` without that digest.

Rust loads whichever review is currently stored for that project and verifies
that review against itself.

With two windows/processes:
1. window A reviews digest A;
2. window B updates the project review to digest B;
3. window A clicks Seal;
4. the backend can seal B even though A displayed A.

Repair:
- Seal & Build receives `expected_review_digest`;
- compare expected digest to the stored current review;
- recompute the draft and compare again;
- reject if any of the three differ;
- frontend must pass `preview.review_digest`.

## P6-FC-03 — Equivalent fact sets are order-sensitive

`apply_reviewed_facts` hashes `facts.join("\n")`.

The same semantic selection in a different click/order sequence therefore gets a
different applicability-context revision and can produce a different mission
hash.

Repair:
- validate;
- sort;
- deduplicate;
- then hash/persist the canonical fact set.

The same semantic fact decisions must produce the same review digest and mission
hash regardless of input ordering or duplicate entries.

## P6-FC-04 — Explicit project deferral does not propagate correctly to tasks/handoff

Project authority requirements are compared in `preseal` against a freshly
constructed seed requirement. After a valid human defer changes status/exception
fields, that exact equality check rejects the project requirement.

Separately, task decomposition occurs before decision state is applied. A
deferred/N-A requirement can therefore retain an `UNSTARTED` task, and
`ExecutionHandoff.task_order` currently contains the full task graph.

P7 must not receive a supposedly executable task for a requirement P6 explicitly
deferred or marked N/A.

Repair:
- validate immutable seed-controlled fields while permitting valid decision-owned
  status/exception changes;
- propagate requirement decision state into task authority;
- tasks whose linked requirements are all deferred/N-A must not be executable;
- `ExecutionHandoff.task_order` must contain only executable P6 tasks;
- mixed tasks remain executable only when they still serve an active applicable
  requirement.

## Closure

After repair run the new closure gates plus all existing P6 gates.

If all pass, produce one final narrow source ZIP. Do not start P7 until the
closure source is independently checked.
