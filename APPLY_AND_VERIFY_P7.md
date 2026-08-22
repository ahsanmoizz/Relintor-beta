# Apply and Verify — Relintor P7 Independent Audit Repair Gate

Overlay this additive package into `D:\Relintor`.

It adds:
- `tooling/evidence/milestone-7-independent-audit.md`
- `crates/relintor-execution/tests/independent_p7_source_audit.rs`
- `tooling/acceptance/independent_p7_desktop_audit.py`

Do not start P8.

First run the failure baseline:

```powershell
Set-Location "D:\Relintor"

$env:RUSTUP_HOME = "D:\Relintor-rustup"
$env:CARGO_HOME = "D:\Relintor-cargo-home"
$env:CARGO_TARGET_DIR = "D:\Relintor\target"
$env:RUSTUP_TOOLCHAIN = "1.96.0-x86_64-pc-windows-msvc"
$env:TEMP = "D:\Relintor-temp"
$env:TMP = "D:\Relintor-temp"

cmd /c "`"D:\DevTools\VSBuildTools\Common7\Tools\VsDevCmd.bat`" -arch=x64 -host_arch=x64 && cargo test -p relintor-execution --test independent_p7_source_audit --locked -- --nocapture"

python tooling/acceptance/independent_p7_desktop_audit.py
```

The audited source is expected to fail these new gates.

Repair production code. Do not weaken the independent tests.

Keep:
- P8 not started;
- GitHub untouched;
- `spec/locked` unchanged;
- P2/P3 external debt truthful;
- real Antigravity smoke pending unless it actually runs.
