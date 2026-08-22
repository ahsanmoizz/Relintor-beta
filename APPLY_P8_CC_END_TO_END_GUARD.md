# Apply P8 CC End-to-End Closure Guard

Overlay into `D:\Relintor`.

Run before repair:

```powershell
cargo test -p relintor-evidence --test independent_p8_cc_end_to_end_guard --locked -- --nocapture
python tooling/acceptance/independent_p8_cc_end_to_end_guard.py
```

Repair only P8-CC-E2E-01 and P8-CC-E2E-02.

Do not start P9. Do not touch GitHub. Do not modify `spec/locked`.
