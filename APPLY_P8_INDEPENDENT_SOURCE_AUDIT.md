# Apply / Verify — Relintor P8 Independent Source Audit Gate

Overlay this ZIP into `D:\Relintor`.

Adds:
- `tooling/evidence/milestone-8-independent-source-audit.md`
- `crates/relintor-evidence/tests/independent_p8_source_audit.rs`
- `tooling/acceptance/independent_p8_source_audit.py`

Do not start P9.

Baseline:

```powershell
Set-Location "D:\Relintor"

$env:RUSTUP_HOME = "D:\Relintor-rustup"
$env:CARGO_HOME = "D:\Relintor-cargo-home"
$env:CARGO_TARGET_DIR = "D:\Relintor\target"
$env:RUSTUP_TOOLCHAIN = "1.96.0-x86_64-pc-windows-msvc"
$env:TEMP = "D:\Relintor-temp"
$env:TMP = "D:\Relintor-temp"

cmd /c "`"D:\DevTools\VSBuildTools\Common7\Tools\VsDevCmd.bat`" -arch=x64 -host_arch=x64 && cargo test -p relintor-evidence --test independent_p8_source_audit --locked -- --nocapture"

python tooling/acceptance/independent_p8_source_audit.py
```

The audited source is expected to fail before repair.

Do not weaken the independent tests/static audit to get green results.
Fix production P8 and then rerun the original P8 43-test corpus plus the full
workspace/desktop/native/security gates.
