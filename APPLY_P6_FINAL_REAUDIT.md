# Apply — P6 Final Independent Re-audit Gate

Overlay this additive package into `D:\Relintor`.

It adds:
- `tooling/evidence/milestone-6-final-independent-reaudit.md`
- `crates/relintor-standards/tests/independent_p6_final_reaudit.rs`
- `tooling/acceptance/independent_p6_final_desktop_audit.py`

Do not start P7.

First run:

```powershell
Set-Location "D:\Relintor"

$env:RUSTUP_HOME = "D:\Relintor-rustup"
$env:CARGO_HOME = "D:\Relintor-cargo-home"
$env:CARGO_TARGET_DIR = "D:\Relintor\target"
$env:RUSTUP_TOOLCHAIN = "1.96.0-x86_64-pc-windows-msvc"
$env:TEMP = "D:\Relintor-temp"
$env:TMP = "D:\Relintor-temp"

cmd /c "`"D:\DevTools\VSBuildTools\Common7\Tools\VsDevCmd.bat`" -arch=x64 -host_arch=x64 && cargo test -p relintor-standards --test independent_p6_final_reaudit --locked -- --nocapture"

python tooling/acceptance/independent_p6_final_desktop_audit.py
```

The audited source is expected to fail these new gates.

Repair production code; do not weaken the tests.

Use a new additive migration if project/takeover binding requires schema changes.
Do not rewrite migrations 005, 006, or 007.
