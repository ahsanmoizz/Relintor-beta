# Relintor

Relintor is the execution and verification layer for AI-built software.

This repository is a fresh implementation. The sealed product authority is
copied verbatim into [`spec/locked`](spec/locked), while
`spec/LOCKED_MANIFEST.sha256` pins the manifest outside the locked copy.

Milestone 1 establishes:

- locked-spec integrity;
- 144-record implementation traceability;
- a Tauri desktop shell;
- Rust-owned local health boundaries;
- a versioned SQLite migration boundary;
- truthful keychain support reporting;
- read-only Antigravity CLI detection;
- cross-platform quality/native-build CI.

## Development

The Rust toolchain is pinned by `rust-toolchain.toml`. JavaScript resolution is
pinned by `pnpm-lock.yaml`. A committed root `Cargo.lock` is required before
native certification.

```text
pnpm install --frozen-lockfile
python tooling/spec/verify_spec.py
python tooling/acceptance/test_spec_verifier.py
python tooling/acceptance/verify_traceability.py
python tooling/acceptance/secret_scan.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
pnpm desktop:typecheck
pnpm desktop:lint
pnpm desktop:test
pnpm desktop:build
pnpm --dir apps/desktop tauri build --no-bundle
```

Milestone 1 does not implement investigation, standards application, mission
execution, evidence verification, subscriptions, AI provider calls, or admin
functionality.

The downloaded `relintor_sealed_product_package/` input folder is not repository
authority and is ignored by Git after its contents have been verified and
copied into `spec/locked/`.
