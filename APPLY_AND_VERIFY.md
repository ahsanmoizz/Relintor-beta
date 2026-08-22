# Apply and verify the Milestone 2 independent repair

Do not start Milestone 3.

1. Overlay this repair pack onto `D:\Relintor`.
2. Do not modify `spec/locked`.
3. Keep D-first Rust/Cargo storage.
4. Inspect `tooling/evidence/milestone-2-independent-audit.md`.
5. Regenerate `Cargo.lock` because the cloud service now has real PostgreSQL and
   UUID dependencies.
6. Format and compile; fix only actual compiler/lint defects without weakening
   the audit requirements.
7. Run all normal regression gates.
8. Run the mandatory live PostgreSQL integration test against a disposable local
   PostgreSQL database.
9. Rewrite `tooling/evidence/milestone-2-verification.md` as a canonical current
   state. Do not append contradictory evidence.

Required commands include:

```text
cargo generate-lockfile
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

cargo test -p relintor-cloud-api mandatory_live_postgres_migration_and_account_round_trip --locked -- --ignored --nocapture

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

For the live PostgreSQL command set:

```text
RELINTOR_TEST_DATABASE_URL
```

to a disposable LOCAL database. This repair's PostgreSQL runtime intentionally
rejects a remote URL while it uses NoTls.

Also run the existing scoped Gitleaks checks and inspect the renderer bundle for
provider secrets.

Final verdict rules:

- `MILESTONE_2_LOCAL_VERIFIED_CROSS_PLATFORM_PENDING` only if every required
  Windows-local gate **including the live PostgreSQL integration gate** passes.
- `MILESTONE_2_IMPLEMENTED_UNVERIFIED` if source compiles/tests but live
  PostgreSQL evidence is missing.
- `MILESTONE_2_FAILED` for an unresolved implementation defect.

macOS/Linux remain deferred and must not be called PASS.

Do not touch GitHub without explicit permission.
Do not start Milestone 3 automatically.
