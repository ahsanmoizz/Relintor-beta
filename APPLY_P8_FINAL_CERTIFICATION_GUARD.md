# Apply — P8 Final Certification Guard

Overlay into `D:\Relintor`.

Adds only:
- `tooling/evidence/milestone-8-final-certification-guard.md`
- `crates/relintor-evidence/tests/independent_p8_final_certification_guard.rs`
- `tooling/acceptance/independent_p8_final_certification_guard.py`

Run before repair:

```powershell
cargo test -p relintor-evidence --test independent_p8_final_certification_guard --locked -- --nocapture
python tooling/acceptance/independent_p8_final_certification_guard.py
```

Repair only P8-CC-01 and P8-CC-02.

Do not start P9, modify spec/locked, or touch GitHub.
