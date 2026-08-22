# Milestone 5 independent audit repair gate

## Current independent verdict

`MILESTONE_5_REPAIR_REQUIRED`

Do not start P6.

## Apply

Overlay this pack onto:

`D:\Relintor`

Read first:

1. `tooling/evidence/milestone-5-independent-audit.md`
2. `crates/relintor-takeover/tests/independent_p5_source_audit.rs`

Do not delete/weaken the independent tests to make the build green.

## Required implementation repairs

Primary file:

`crates/relintor-takeover/src/lib.rs`

Also update persistence/migrations/report/traceability where genuinely needed.

### A. Probe authority

`execute_read_only` must not trust a caller-supplied `ProbeSafety`.

A forged `SafeReadOnly` object must not be able to execute:

- cargo build
- cargo test
- node scripts
- pnpm scripts
- arbitrary repository code

Use either:

- a typed/unforgeable read-only probe operation; or
- strict exact-shape validation at execution time.

### B. Runtime P5 build/test evidence

The initial takeover scan stays read-only.

Add a separate controlled fixture/sandbox execution boundary for P5 verification.

For the seeded P5 corpus:

- failing test must actually run and exit nonzero;
- broken build must actually run and exit nonzero;
- stdout/stderr/exit code/timestamps/workspace fingerprint must be captured;
- imported source must not be modified;
- runtime failure must be reconciled into BROKEN.

Do not run arbitrary real user project scripts just to pass this gate.

A disposable copied fixture is sufficient for acceptance.

### C. Repository fingerprint

Stream SHA-256 over the full included file contents for fingerprint purposes.

`max_file_bytes` / analysis-text limits may still prevent large content from
being retained for parsing, but they must not make same-size content changes
invisible to the repository fingerprint.

### D. Dependency graph

Implement deterministic:

- internal workspace edges;
- missing `workspace:` references;
- cycle detection where supported.

Do not claim those fields are populated while returning constant empty vectors.

### E. Migration correlation

Do not attach every discovered migration to every schema object.

Correlate migration evidence to object/table/model names where deterministic.

A repository with one migrated table and one unmigrated table must still produce
missing-migration evidence.

### F. Dead-code truthfulness

Filename substrings such as `dead` / `orphan` cannot by themselves prove DEAD.

Use stronger reachability/reference evidence where supported.

If evidence is only heuristic, use UNPROVEN / possible-dead wording rather than
DEAD.

### G. Auth truthfulness

Separate:

AUTH_LIBRARY_PRESENT
AUTH_IMPLEMENTED
ROUTE_PROTECTED
AUTHORIZATION_PROVEN
AUTHORIZATION_UNPROVEN

Ignore comment-only keyword evidence for AuthImplemented.

### H. Revisions

Preserve changed takeover scans as durable revisions.

Do not permanently replace revision 1 on every changed scan.

If schema changes are needed, add an additive migration rather than rewriting
005 after it may have been applied.

### I. Reparse/symlink evidence

Add an OS-appropriate test for escaping through symlink/reparse/junction
behavior.

If the environment cannot create the relevant Windows object, report that
specific test as NOT_RUN rather than PASS.

## Required test execution

Use the existing D-first MSVC environment.

Run:

```text
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

cargo test -p relintor-takeover --test independent_p5_source_audit --locked -- --nocapture
cargo test -p relintor-takeover --locked
cargo test --workspace --locked

python tooling/spec/verify_spec.py
python tooling/acceptance/test_spec_verifier.py
python tooling/acceptance/verify_traceability.py
python tooling/acceptance/secret_scan.py

pnpm install --frozen-lockfile
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
pnpm --dir apps/desktop test
pnpm --dir apps/desktop build
pnpm --dir apps/desktop tauri build --no-bundle
```

Use the existing Gitleaks executable:

`D:\Relintor-cargo-home\gitleaks-tools\gitleaks-8.30.1\gitleaks.exe`

Run the existing 13 scoped scans plus renderer provider-secret inspection.

## Traceability

Reset/keep affected P5 verification `not_run` until matching repaired evidence
passes.

At minimum re-evaluate:

- B-02
- B-05
- B-07
- B-09
- B-12

Do not mark P6/P7/P8 implemented.

## Canonical report

Rewrite:

`tooling/evidence/milestone-5-verification.md`

Do not append contradictory historical state.

Report separately:

- STATIC evidence;
- CONTROLLED RUNTIME evidence;
- RUNTIME_UNPROVEN evidence.

Do not describe marker-only evidence as an executed failing build/test.

## Verdict

Return:

`MILESTONE_5_LOCAL_VERIFIED_CROSS_PLATFORM_PENDING`

only if all independent source-audit gates and normal Windows-local gates pass.

Otherwise:

`MILESTONE_5_IMPLEMENTED_UNVERIFIED`

for missing execution/environment evidence,

or:

`MILESTONE_5_FAILED`

for unresolved implementation failures.

Keep P2/P3/P4 carried debt unchanged.

Do not touch GitHub.
Do not start P6.
