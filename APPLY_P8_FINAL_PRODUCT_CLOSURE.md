# Apply — P8 Final Product/Authority Closure Gate

Overlay this ZIP into `D:\Relintor`.

Adds:
- `tooling/evidence/milestone-8-final-product-closure-audit.md`
- `crates/relintor-evidence/tests/independent_p8_final_product_closure.rs`
- `tooling/acceptance/independent_p8_final_product_closure.py`

P9 must remain NOT STARTED.

Baseline:

```powershell
Set-Location "D:\Relintor"

$env:RUSTUP_HOME = "D:\Relintor-rustup"
$env:CARGO_HOME = "D:\Relintor-cargo-home"
$env:CARGO_TARGET_DIR = "D:\Relintor\target"
$env:RUSTUP_TOOLCHAIN = "1.96.0-x86_64-pc-windows-msvc"
$env:TEMP = "D:\Relintor-temp"
$env:TMP = "D:\Relintor-temp"

cmd /c "`"D:\DevTools\VSBuildTools\Common7\Tools\VsDevCmd.bat`" -arch=x64 -host_arch=x64 && cargo test -p relintor-evidence --test independent_p8_final_product_closure --locked -- --nocapture"

python tooling/acceptance/independent_p8_final_product_closure.py
```

The audited certification delta is expected to fail before the focused repairs.

Do not weaken the independent gate or replace real collectors with PASS DTOs.
