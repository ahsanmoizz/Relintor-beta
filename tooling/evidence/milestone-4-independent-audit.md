# Relintor Milestone 4 — Independent Source Audit

Date: 2026-08-14

## Verdict

`MILESTONE_4_REPAIR_REQUIRED`

This verdict comes from inspection of the uploaded Milestone 4 source archive,
not from the implementer's closure summary.

The implementation contains a substantial and useful P4 foundation, but the
current `MILESTONE_4_LOCAL_VERIFIED_CROSS_PLATFORM_PENDING` claim is too strong.

## What is genuinely present

The source contains real typed P4 structures for:

- project drafts and investigation state;
- provenance-bearing source documents;
- intent claims;
- personas and user journeys;
- NFRs;
- investigator questions and options;
- assumptions, risks, ADRs and conflicts;
- blueprint revisions and approvals as domain types;
- candidate standards/requirements/evidence;
- deterministic blueprint fingerprints;
- SQLite P4 tables;
- a deterministic investigator provider;
- a desktop idea/question review surface.

The five sealed P4 concepts are represented in code. The problem is that several
important behaviors are either incomplete or were not actually verified by the
tests that the canonical report claims verified them.

## Blocking findings

### 1. Selected user answers do not materially drive the blueprint

`crates/relintor-investigator/src/lib.rs`

`build_questions` changes an answered question from `Asked` to `Answered`, but
the selected option is not applied to intent, deployment, architecture
decisions, NFRs, capabilities, risks, or another semantic blueprint record.

A user can choose materially different options and the implementation mostly
changes only question disposition.

This conflicts with the investigator purpose: user answers must affect the
blueprint when the answer changes architecture, scope, security, cost,
deployment or verification.

### 2. `apply_answer` discards earlier answers

`crates/relintor-investigator/src/lib.rs`

The method begins with a new vector containing only the latest answer:

```text
let mut answers = vec![answer.clone()];
```

It does not merge `result.answers`.

A multi-question review can therefore revert a previously answered question when
the core revision API is used.

The method is not called by any test in the submitted source.

### 3. Conflict detection is not across idea/docs/answers

`detect_conflicts` scans only `SourceDocument.extracted_text`.

It does not inspect:

- the original typed idea;
- selected user answers.

The sealed feature register names C-11 as conflict detection across
docs/answers.

A direct example from the submitted ten-case corpus is:

- idea: `A local-only project`
- document: `... cloud synchronization`

The implementation does not detect that idea-vs-document deployment conflict,
because `local` is searched only in source documents.

### 4. The "diverse corpus" was not actually executed as a corpus gate

The submitted test:

```text
corpus_covers_ten_meaningfully_different_projects
```

only asserts that two collections contain ten items.

It does not run all ten cases through the investigator and compare the produced
intent/questions/conflicts against expected behavior.

Therefore the sealed P4 requirement "verify with a diverse project corpus" is
not proven by this test.

### 5. Revision behavior is claimed as tested but the revision API is untested

The verification report says the investigator tests cover revision behavior.

Static source inspection shows `apply_answer(` occurs only at its definition.
No submitted test invokes it.

The approval test manually constructs a `BlueprintRevision`; it does not test
answer -> revision -> persistence -> approval.

### 6. Approval is not an ordinary-user product flow and is not persisted

The migration creates `blueprint_approvals`.

No runtime source writes to `blueprint_approvals`.

`BlueprintApproval` exists as a type and `Investigator::approve` returns an
in-memory record, but there is no Tauri approval command and no persistence
function for the approval record.

The desktop button is explicitly disabled:

```text
Approval follows answer review
```

C-12/K-04 may remain partial, but their verification cannot be treated as a
real founder-approval flow.

### 7. Multi-document intake exists as a library API but is not wired to desktop

`IntakeService::intake_files` exists.

No submitted source calls it.

The Tauri new-project command accepts only:

```text
idea: String
```

The desktop UI has no attachment control.

The canonical report itself acknowledges that the desktop multi-document picker
remains follow-on work.

B-04 can remain partially implemented, but a full P4 product-flow verification
should not claim the desktop source path is complete.

### 8. Explicit typed-idea claims can have empty provenance

Inside `investigate`, `source_provenance` is built only from attached
`SourceDocument`s.

`build_intent` then marks the typed project idea as `ExplicitFact` while using
that source-only provenance vector.

For a project created with only an idea and no documents, the explicit product
purpose claim therefore has no provenance even though `ProjectDraft` already
contains correct typed-idea provenance.

This breaks the "important claims retain provenance" design.

### 9. Desktop answer provenance does not identify the actual answer

`apps/desktop/src-tauri/src/lib.rs`

Every answer uses:

```text
content_hash = sha256("user-answer")
```

regardless of which option was selected.

The answer ID is also based only on question ID.

Changing an answer can therefore reuse the same identity/hash and persistence
uses `INSERT OR REPLACE`, which weakens history and provenance.

### 10. Several claimed additional gates do not have matching tests

In the submitted investigator source:

- `intake_files(` occurs only at its definition;
- `apply_answer(` occurs only at its definition;
- `PathOutsideAllowedRoot` has implementation references but no test;
- `ConflictResolutionState::Resolved` is never constructed;
- `blueprint_approvals` appears only in the SQL migration.

The canonical report's statement that all of these behaviors were covered is
therefore too broad.

### 11. Pure backend/API projects still receive an accessibility NFR/domain

`build_nfrs` always adds an accessibility requirement.

`build_candidate_domains` always adds accessibility.

For a headless API/backend project this is exactly the kind of irrelevant
requirement P4/P6 architecture is meant to avoid. Applicability must remain
evidence-driven.

### 12. AI gateway integration is a provider abstraction, not an actual gateway call

`Investigator::investigate` calls:

```text
self.provider.analyze(...)
```

directly.

The deterministic development provider is truthfully labeled, which is good,
but there is no runtime call from the investigator to `services/ai-gateway`.

The current implementation should be described as a local deterministic
provider boundary prepared for a future gateway adapter, not as completed AI
gateway integration.

The "malformed provider output" test also tests a provider returning `Err`; it
does not parse and reject malformed structured provider payload.

## Traceability impact

The implementation remains useful and should not be thrown away.

However the affected P4 records should return to `verification_status=not_run`
until the independent acceptance tests pass after repair.

The implementation status can remain `partially_implemented`.

Affected records:

- B-01
- B-04
- C-01 through C-12
- K-03
- K-04

## Certification rule

Milestone 4 can return to:

`MILESTONE_4_LOCAL_VERIFIED_CROSS_PLATFORM_PENDING`

only after:

1. selected answers materially affect structured blueprint state;
2. sequential answers preserve prior answers;
3. invalid question/option IDs fail closed;
4. conflicts include idea/docs/answers;
5. the full diverse corpus is actually executed;
6. idea provenance is retained;
7. revision behavior is tested;
8. approval is either wired/persisted or C-12/K-04 are truthfully left
   unverified/partial without claiming the complete approval flow;
9. multi-document intake is actually exercised;
10. allowed-root traversal is tested;
11. irrelevant NFR/domain suppression is demonstrated;
12. all normal Windows regression gates pass again.

P2/P3 carried debt remains unchanged and is unrelated to this P4 repair.
