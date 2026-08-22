# Relintor Milestone 5 — Independent Source Audit

Date: 2026-08-14

## Verdict

`MILESTONE_5_REPAIR_REQUIRED`

This verdict comes from inspection of the uploaded Milestone 5 source archive,
not from the implementer's canonical report.

The P5 implementation is substantial and useful, but several important claims
in the current verification report are stronger than the source and tests
support.

## What is genuinely present

The uploaded source contains real P5 foundations for:

- read-only repository inventory;
- build-system detection;
- package/Cargo dependency discovery;
- route/API discovery;
- database/schema/migration discovery;
- auth/permission hints;
- UI/test/CI/deployment discovery;
- documentation promise extraction;
- capability/finding/recommendation models;
- static-vs-runtime evidence types;
- runtime-probe policy primitives;
- SQLite takeover persistence;
- desktop Existing Project / Take Over Project flow;
- a seeded takeover corpus.

The source also correctly avoids running repository scripts during the initial
static scan.

## Blocking findings

### 1. The sealed broken-build and failing-test gates were not runtime gates

Source:
`crates/relintor-takeover/src/lib.rs`

The reconciliation engine treats fixture/source markers as deterministic
failure:

- failing test: around line 2092;
- broken build: around line 2138.

The seeded broken-build fixture contains a marker and a package script with
`exit 1`; the failing-test fixture contains a marker/obvious failing source.

However the P5 acceptance suite never executes those project test/build commands
inside an isolated copy.

The current runtime API executes only `SafeReadOnly` probes. A planned build is
classified `SandboxRequired`, but there is no sandbox executor that runs the
seeded build/test and feeds the result back into capability reconciliation.

Therefore the report's claim that the seeded project whose "actual build command
fails" was proven is too strong.

Required repair:

- retain static evidence as static evidence;
- add a controlled disposable/sandbox fixture execution path;
- execute the seeded failing test and broken build;
- attach stdout/stderr/exit-code/runtime fingerprint;
- ensure deterministic runtime failure is what produces/locks `BROKEN`.

Do not execute imported user repositories in place.

### 2. A caller can forge a `SafeReadOnly` probe and execute a build

`RuntimeProbe` has public fields (around line 332).

`execute_read_only` (around line 2371) trusts:

```text
probe.safety == SafeReadOnly
```

and then only calls the generic `validate_command`.

A caller can construct:

```text
exact_command = ["cargo", "build"]
safety = SafeReadOnly
```

`validate_command` permits `cargo build`, so `execute_read_only` will spawn it.

That violates the fail-closed rule because Cargo builds may execute repository
build scripts.

The execution method must validate the exact operation itself, not trust a
caller-controlled enum.

At minimum a read-only version probe should require the exact approved shape:

```text
<allowlisted executable> --version
```

or use an unforgeable typed probe kind.

### 3. Repository fingerprints do not detect same-size changes to large files

Around line 808, files larger than `max_hash_bytes` get:

```text
content_hash = None
```

The repository fingerprint later hashes:

```text
relative path + size + content_hash.unwrap_or_default()
```

Therefore changing a large file without changing its byte length can leave the
repository fingerprint unchanged.

That breaks the required stale-evidence invalidation boundary.

Repair by streaming a full content hash for the repository fingerprint even when
the file is too large to retain/read as analysis text.

"Do not load into memory" and "do not fingerprint" are different concerns.

### 4. Dependency graph claims are not actually implemented

Around lines 1277-1278:

```text
internal_edges: Vec::new()
cycles: Vec::new()
```

`missing_reference` is also initialized false and never meaningfully populated.

Yet the canonical report says the graph records:

- internal edges;
- cycles;
- missing internal references.

That report statement is false for the submitted source.

Implement at least deterministic workspace/package internal edges, missing
workspace references, and cycle detection, or lower the verification claim.

### 5. Missing-migration detection fails when any migration exists

Around line 1615:

```text
if !migrations.is_empty() {
    for object in &mut schema_objects {
        object.migration_evidence = all migrations;
        object.migration_required = false;
    }
}
```

So if a repository has:

- a migration for `users`;
- a model/schema for `payments`;
- no migration for `payments`;

the scanner marks every schema object as having migration evidence.

The current seeded fixture contains zero migrations, so it does not test this
important case.

Migration evidence must be correlated to the actual schema/model object where
deterministically possible.

### 6. Dead-code classification is too aggressive

Around line 2004, a source file whose path merely contains:

```text
dead
```

or:

```text
orphan
```

can cause a `DEAD` classification.

A valid source file named:

```text
deadline.ts
```

matches `dead`.

This violates the P5 rule that uncertain dead-code evidence should be
`UNPROVEN`/possible rather than deterministically `DEAD`.

A filename heuristic can generate an investigation lead, but it cannot by itself
prove dead reachability.

### 7. Auth implementation is overclaimed from keywords/comments

Around line 1680, when a file contains terms such as OAuth/JWT/session, the
scanner emits both:

```text
AuthLibraryPresent
AuthImplemented
```

The entire raw file is scanned, including comments and documentation-like
strings.

A comment saying "TODO: add OAuth" can therefore become `AuthImplemented`.

The required states exist, but the detector is not currently strong enough to
separate them.

### 8. Takeover revision history is not durable

Every scan creates:

```text
revision: 1
```

The persistence layer uses `INSERT OR REPLACE`, and the database has:

```text
UNIQUE(takeover_id, revision)
```

Scanning the same project after it changes therefore replaces revision 1 rather
than preserving a revision sequence.

The report exposes a `TakeoverRevision` model, but actual historical revision
persistence is not working as that model implies.

### 9. Symlink/reparse safety was claimed but not acceptance-tested

The scanner explicitly skips entries for which:

```text
metadata.file_type().is_symlink()
```

which is good.

However the submitted independent acceptance tests contain no actual symlink /
Windows reparse-point escape fixture.

Because P5 reports reparse protection as verified, add an OS-appropriate
acceptance test. If a specific Windows junction/reparse type cannot be safely
created in the test environment, report that portion as `NOT_RUN` rather than
PASS.

### 10. Several P5 verification statements need narrower wording

The current implementation is a static discovery foundation. Some detectors are
heuristic:

- auth;
- duplicate implementation;
- UI journey reconstruction;
- dead-code inference;
- route reachability.

These are still useful, but the canonical report should distinguish:

```text
DETECTED_HINT
STATIC_EVIDENCE
RUNTIME_PROVEN
```

and avoid language implying stronger proof than exists.

## Traceability disposition

Keep implementation status conservative.

Until the independent repair gates pass, reset verification to `not_run` for:

- B-02 Existing Project Takeover
- B-05 Repository inventory
- B-07 Dependency graph discovery
- B-09 Database/schema discovery
- B-12 Capability reality classification

Other P5 records may remain as implemented/verified only if their exact existing
evidence remains valid after reconciliation.

Do not upgrade any P6/P7/P8 records.

## Certification rule

P5 may return to:

`MILESTONE_5_LOCAL_VERIFIED_CROSS_PLATFORM_PENDING`

only after:

1. forged SafeReadOnly probes cannot execute build/test/script operations;
2. large same-size content changes alter repository fingerprint;
3. dependency internal edges/missing refs/cycles are real or the claim is
   truthfully reduced;
4. migration evidence is object-correlated;
5. filename-only dead-code evidence cannot become deterministic DEAD;
6. comments/keywords cannot by themselves become AuthImplemented;
7. seeded broken-build/failing-test gates are backed by controlled runtime
   evidence, not only markers;
8. repeated changed scans preserve takeover revision history;
9. symlink/reparse behavior is actually tested or truthfully marked not-run;
10. all normal Windows regression/security gates pass.

P2/P3/P4 carried debt remains unchanged.
