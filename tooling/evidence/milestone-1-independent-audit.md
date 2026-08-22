# Milestone 1 Independent Audit — 2026-08-13

## Verdict before repair

`MILESTONE_1_REPAIR_REQUIRED`

The original implementation was directionally strong and correctly refused to
claim native/cross-platform verification. Independent source review found
implementation defects that are separate from the reported network/platform
blockers.

## Release-blocking source defects found

1. Installed builds did not bundle the sealed specification authority.
2. Desktop spec health used a compile-time workspace path and therefore could
   fail or inspect the wrong location after installation.
3. Rust spec health verified the mutable manifest but did not verify the
   externally pinned manifest hash, allowing a modified manifest plus modified
   files to appear healthy in the desktop UI.
4. Antigravity detection omitted the official `agy` CLI executable name.
5. The serialized desktop database health contract used `Result` inside the
   response object while TypeScript expected an object/string shape.
6. A health command invoked the database migration, so a nominally read-only
   health read could mutate local state.
7. Windows/macOS keychain status claimed `available` without a secure-store
   round-trip probe.
8. Traceability incorrectly marked K-03 Guided investigator UI as
   `foundation_only` even though the milestone explicitly did not implement
   investigator UI.
9. Cross-platform CI did not compile the native Tauri shell and lacked Linux
   Tauri prerequisites.
10. CI allowed the pnpm lockfile to change and did not require a `Cargo.lock`.
11. The UI could remain stuck on `checking` in browser preview.
12. `body { min-width: 960px; }` undermined the sealed 200% zoom/reflow
    accessibility requirement.
13. Production CSP allowed the development localhost origin and capabilities
    were broader/less explicit than needed.

## Repair state

The repair pack corrects the source/configuration defects above, but it is
**not** itself a certification. The coding environment must apply these files,
generate and commit `Cargo.lock` after dependency access is restored, and run
the complete verification matrix.

## Required post-repair evidence

- `python tooling/spec/verify_spec.py`
- `python tooling/acceptance/test_spec_verifier.py`
- `python tooling/acceptance/verify_traceability.py`
- `python tooling/acceptance/secret_scan.py`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- `pnpm --dir apps/desktop typecheck`
- `pnpm --dir apps/desktop lint`
- `pnpm --dir apps/desktop test`
- `pnpm --dir apps/desktop build`
- `pnpm --dir apps/desktop tauri build --no-bundle`
- GitHub Actions matrix results for Windows, macOS and Linux
- Gitleaks result

Until those are run successfully, the correct status remains
`MILESTONE_1_IMPLEMENTED_UNVERIFIED`.
