# Apply — Relintor P6 Final Closure Gate

Overlay this additive ZIP into `D:\Relintor`.

It adds:
- `tooling/evidence/milestone-6-final-closure-audit.md`
- `crates/relintor-standards/tests/independent_p6_closure_audit.rs`
- `tooling/acceptance/independent_p6_closure_desktop_audit.py`

Do not start P7.

Run the baseline:

```powershell
Set-Location "D:\Relintor"

$env:RUSTUP_HOME = "D:\Relintor-rustup"
$env:CARGO_HOME = "D:\Relintor-cargo-home"
$env:CARGO_TARGET_DIR = "D:\Relintor\target"
$env:RUSTUP_TOOLCHAIN = "1.96.0-x86_64-pc-windows-msvc"
$env:TEMP = "D:\Relintor-temp"
$env:TMP = "D:\Relintor-temp"

cmd /c "`"D:\DevTools\VSBuildTools\Common7\Tools\VsDevCmd.bat`" -arch=x64 -host_arch=x64 && cargo test -p relintor-standards --test independent_p6_closure_audit --locked -- --nocapture"

python tooling/acceptance/independent_p6_closure_desktop_audit.py
```

The current audited source is expected to fail.

Repair P6-FC-01 through P6-FC-04 only. Do not weaken the independent tests.
