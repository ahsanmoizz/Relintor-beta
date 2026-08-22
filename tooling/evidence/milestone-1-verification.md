# Relintor Milestone 1 Verification Record

Date: 2026-08-14  
Repository: `D:\Relintor`

## CURRENT STATE

Verdict: `MILESTONE_1_LOCAL_VERIFIED_CROSS_PLATFORM_PENDING`

All requested Windows-local gates passed in the x64 MSVC developer environment.
macOS and Linux have not run on hosted runners, so this is not a
`MILESTONE_1_VERIFIED` result. Milestone 2 was not started. GitHub was not
touched. `spec/locked` was not modified.

## D:-first environment and storage

```text
Repository:       D:\Relintor
RUSTUP_HOME:      D:\Relintor-rustup
CARGO_HOME:       D:\Relintor-cargo-home
CARGO_TARGET_DIR: D:\Relintor\target
```

Free space after testing:

| Drive | Free bytes | Approximate free space |
|---|---:|---:|
| C: | 1,745,186,816 | 1.63 GiB |
| D: | 93,087,272,960 | 86.69 GiB |

`Cargo.lock` exists at `D:\Relintor\Cargo.lock` and was used by all locked
Cargo commands. `pnpm-lock.yaml` remained frozen and unchanged.

## Visual Studio and Rust evidence

Visual Studio developer environment:

```text
D:\DevTools\VSBuildTools\Common7\Tools\VsDevCmd.bat
```

The script was sourced with `-arch=x64 -host_arch=x64`.

Resolved tools:

```text
cl.exe:   D:\DevTools\VSBuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\cl.exe
link.exe: D:\DevTools\VSBuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\link.exe
```

Rust:

```text
rustc 1.96.0 (ac68faa20 2026-05-25)
host: x86_64-pc-windows-msvc
release: 1.96.0
```

The active repository override is `1.96.0-x86_64-pc-windows-msvc`.

## Exact command ledger

All commands ran from `D:\Relintor`. Rust commands ran after sourcing the x64
MSVC developer environment with the D: environment variables above.

| Exact command | Exit code | Status | Result |
|---|---:|---|---|
| `rustup show` | 0 | PASS | D: rustup home; repository override active; MSVC host. |
| `rustup toolchain list` | 0 | PASS | Pinned MSVC toolchain active. |
| `rustc -vV` | 0 | PASS | Host `x86_64-pc-windows-msvc`. |
| `where cl` | 0 | PASS | x64 MSVC compiler resolved at the path above. |
| `where link` | 0 | PASS | x64 MSVC linker resolved at the path above. |
| `cargo fmt --all -- --check` | 0 | PASS | Formatting clean under MSVC. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | PASS | Clippy completed with `-D warnings`. |
| `cargo test --workspace --locked` | 0 | PASS | 6 unit tests passed; all other workspace targets and doc-tests passed with 0 tests. |
| `cargo test -p relintor-core --locked` | 0 | PASS | 6/6 tests passed; 0 failed. |
| `pnpm --dir apps/desktop tauri build --no-bundle` | 0 | PASS | Native MSVC build produced `D:\Relintor\target\release\relintor-desktop.exe`. |
| `python tooling/spec/verify_spec.py` | 0 | PASS | `files=18`, `manifest_entries=17`, `features=144`, `unique_ids=144`. |
| `python tooling/acceptance/test_spec_verifier.py` | 0 | PASS | All baseline and expected tamper failures passed. |
| `python tooling/acceptance/verify_traceability.py` | 0 | PASS | `records=144`, `unique_ids=144`. |
| `python tooling/acceptance/secret_scan.py` | 0 | PASS | No provider/private-key/connection-secret patterns. |
| `pnpm --dir apps/desktop typecheck` | 0 | PASS | TypeScript check passed. |
| `pnpm --dir apps/desktop lint` | 0 | PASS | ESLint passed. |
| `pnpm --dir apps/desktop test` | 0 | PASS | 1 file, 4 tests passed; React `act(...)` warnings only. |
| `pnpm --dir apps/desktop build` | 0 | PASS | TypeScript/Vite production build passed. |

Scoped Gitleaks commands also passed with exit code 0 for each path:

```text
gitleaks dir --redact --no-banner --exit-code 1 .github
gitleaks dir --redact --no-banner --exit-code 1 apps/desktop
gitleaks dir --redact --no-banner --exit-code 1 crates
gitleaks dir --redact --no-banner --exit-code 1 db
gitleaks dir --redact --no-banner --exit-code 1 integrations
gitleaks dir --redact --no-banner --exit-code 1 packages
gitleaks dir --redact --no-banner --exit-code 1 services
gitleaks dir --redact --no-banner --exit-code 1 tooling
gitleaks dir --redact --no-banner --exit-code 1 Cargo.toml
gitleaks dir --redact --no-banner --exit-code 1 package.json
gitleaks dir --redact --no-banner --exit-code 1 pnpm-workspace.yaml
gitleaks dir --redact --no-banner --exit-code 1 rust-toolchain.toml
gitleaks dir --redact --no-banner --exit-code 1 REPAIR_MANIFEST.json
```

Gitleaks result: no leaks found in all scoped paths.

## Full Cargo workspace test totals

`cargo test --workspace --locked` exit code: `0`.

- Unit-test binaries: 8 total; 1 binary ran 6 tests; 7 binaries ran 0 tests.
- Unit tests: 6 passed, 0 failed, 0 ignored, 0 measured, 0 filtered out.
- Doc-test targets: 7 total; 0 passed, 0 failed, 0 ignored, 0 measured.
- Overall executed test count: 6 passed, 0 failed.

`cargo test -p relintor-core --locked` independently reproduced 6 passed and 0
failed tests.

## Current Windows matrix

| Windows gate | Status |
|---|---|
| Rust host triple | PASS |
| x64 MSVC developer environment | PASS |
| `cl.exe` and `link.exe` resolution | PASS |
| Spec verification and acceptance | PASS |
| Traceability | PASS |
| Secret scan | PASS |
| Cargo format | PASS |
| Cargo Clippy | PASS |
| Full workspace tests | PASS |
| `relintor-core` tests | PASS |
| Native Tauri no-bundle build | PASS |
| TypeScript typecheck | PASS |
| ESLint | PASS |
| Vitest | PASS — 4/4 |
| Vite production build | PASS |
| Scoped Gitleaks | PASS |
| Windows local Milestone 1 closure | PASS |

## macOS/Linux matrix

| Platform | Status | Evidence |
|---|---|---|
| macOS | BLOCKED_ENVIRONMENT / NOT_RUN | No hosted runner executed. |
| Linux | BLOCKED_ENVIRONMENT / NOT_RUN | No hosted runner executed. |

The repository has no remote configured. No GitHub Actions workflow was
triggered, and no macOS/Linux certification is claimed.

## Files changed

Repository files changed during this closure:

- `apps/desktop/src-tauri/build.rs` — restored the final newline required by
  `cargo fmt --all -- --check`.
- `tooling/evidence/milestone-1-verification.md` — rewritten as this canonical
  current-state report.

No file under `spec/locked` changed. `Cargo.lock` and `pnpm-lock.yaml` were not
modified during this closure.

## Exact remaining blocker

Only cross-platform hosted verification remains: macOS and Linux required gates
must execute on actual runners. Until those runners execute successfully, the
correct verdict remains:

```text
MILESTONE_1_LOCAL_VERIFIED_CROSS_PLATFORM_PENDING
```

## Historical attempts / superseded evidence

The earlier GNU/MinGW linker failure is retained only as historical context:

```text
x86_64-w64-mingw32-gcc
ld.exe: error: export ordinal too large: 92018
collect2.exe: error: ld returned 1 exit status
```

An earlier formatting check also exited `1` because `build.rs` lacked a final
newline; that source defect was corrected and the required final formatting check
passed. These superseded attempts are not current status.
