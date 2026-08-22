# Apply and Verify — Relintor P6 Independent Audit Repair Gate

This ZIP is additive. It does not contain replacement production source.

Copy these files into `D:\Relintor`:

- `tooling\evidence\milestone-6-independent-audit.md`
- `crates\relintor-standards\tests\independent_p6_source_audit.rs`

Then give the audit report to Luna and repair the production implementation.

## First command after the files exist

Use the existing D:-first Rust/MSVC environment and run:

```powershell
Set-Location "D:\Relintor"

$env:RUSTUP_HOME = "D:\Relintor-rustup"
$env:CARGO_HOME = "D:\Relintor-cargo-home"
$env:CARGO_TARGET_DIR = "D:\Relintor\target"
$env:RUSTUP_TOOLCHAIN = "1.96.0-x86_64-pc-windows-msvc"
$env:TEMP = "D:\Relintor-temp"
$env:TMP = "D:\Relintor-temp"

cmd /c "`"D:\DevTools\VSBuildTools\Common7\Tools\VsDevCmd.bat`" -arch=x64 -host_arch=x64 && cargo test -p relintor-standards --test independent_p6_source_audit --locked -- --nocapture"
```

The current audited source is expected to FAIL this independent gate.

Do not weaken/remove the independent tests to make them pass.

## Repair rules

- Do not modify `spec/locked`.
- Do not start P7.
- Do not touch GitHub.
- Keep P2/P3 carried debt unchanged.
- Do not rewrite already-applied migration 006. Use a new additive migration for
  persistence-key/schema repair.
- Keep signing private keys out of source, renderer, database and bundles.
- The production desktop must receive only trusted public verification material.
- Preserve P4/P5 history; integrate it into P6 rather than replacing it.

## Closure

After production repairs:

1. independent P6 source audit target: PASS;
2. dedicated P6 acceptance: PASS;
3. full workspace Rust: PASS;
4. Python spec/traceability/secret gates: PASS;
5. desktop typecheck/lint/test/build: PASS;
6. native Tauri build: PASS;
7. Gitleaks 13 scopes: PASS;
8. renderer/provider/signing-secret scan: PASS.

Then update `tooling/evidence/milestone-6-verification.md` truthfully and stop.
The next step is another independent source audit before P7.
