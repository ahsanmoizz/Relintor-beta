# Apply / repair / verify P4

## Important

This pack is an **independent repair gate**, not a claim that the implementation
has already been corrected.

Overlay it onto `D:\Relintor`.

It adds independent behavioral tests and resets affected P4 verification claims
to `not_run`.

Do not start P5.

## Required source repairs

Repair the actual implementation, especially:

- `crates/relintor-investigator/src/lib.rs`
- `apps/desktop/src-tauri/src/lib.rs`
- `apps/desktop/src/App.tsx`
- `apps/desktop/src/backend.ts`
- `db/migrations/004_investigator_foundation.sql` only if persistence behavior
  genuinely requires an additive correction; prefer a new additive migration
  rather than rewriting already-applied history.

Required outcomes:

1. Merge prior answers when applying a new answer.
2. Validate question IDs and selected option IDs.
3. Material selected answers must update structured blueprint state/ADRs.
4. Include typed-idea provenance in explicit idea-derived claims.
5. Detect material conflicts across idea + documents + user answers.
6. Exercise the actual ten-case corpus, not only fixture counts.
7. Make NFR/candidate-domain relevance project-sensitive.
8. Make answer identity/hash depend on the actual answer payload.
9. Preserve answer/revision history rather than overwriting changed answers.
10. Either implement persistence + ordinary-user approval/revision correctly, or
    keep C-12/K-04 truthfully partial/unverified.
11. Exercise real multi-document intake and allowed-root rejection in tests.
12. Do not claim production AI-gateway integration until a real gateway adapter
    exists; the deterministic adapter must remain truthfully labeled.

## Run

Use the existing D-first MSVC environment, then run at minimum:

```text
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test -p relintor-investigator --test independent_p4_acceptance --locked

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

Run the existing scoped Gitleaks and renderer-secret checks.

## Verdict rule

Return:

`MILESTONE_4_LOCAL_VERIFIED_CROSS_PLATFORM_PENDING`

only when:

- all independent P4 acceptance tests pass;
- the five sealed P4 gates still pass;
- the normal Windows regression matrix passes;
- traceability is reconciled to actual evidence.

Otherwise use:

`MILESTONE_4_IMPLEMENTED_UNVERIFIED`

for missing evidence, or:

`MILESTONE_4_FAILED`

for unresolved implementation failures.

Do not modify `spec/locked`.

Do not touch GitHub.

Do not start P5 automatically.

P2/P3 carried debt remains unchanged.
