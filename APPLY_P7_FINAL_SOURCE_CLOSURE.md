# Apply — P7 Final Source Closure Gate

Overlay this ZIP into `D:\Relintor`.

Adds:
- `tooling/evidence/milestone-7-final-source-closure-audit.md`
- `crates/relintor-execution/tests/independent_p7_final_source_closure.rs`
- `tooling/acceptance/independent_p7_final_source_closure.py`

P8 must remain not started.

Baseline:

```powershell
Set-Location "D:\Relintor"

$env:RUSTUP_HOME = "D:\Relintor-rustup"
$env:CARGO_HOME = "D:\Relintor-cargo-home"
$env:CARGO_TARGET_DIR = "D:\Relintor\target"
$env:RUSTUP_TOOLCHAIN = "1.96.0-x86_64-pc-windows-msvc"
$env:TEMP = "D:\Relintor-temp"
$env:TMP = "D:\Relintor-temp"

cmd /c "`"D:\DevTools\VSBuildTools\Common7\Tools\VsDevCmd.bat`" -arch=x64 -host_arch=x64 && cargo test -p relintor-execution --test independent_p7_final_source_closure --locked -- --nocapture"

python tooling/acceptance/independent_p7_final_source_closure.py
```

The audited source is expected to fail before the four focused repairs.
Do not weaken the independent tests.
