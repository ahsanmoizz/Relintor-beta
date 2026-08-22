from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[2]
tauri = (root / "apps/desktop/src-tauri/src/lib.rs").read_text(encoding="utf-8")
execution = (root / "crates/relintor-execution/src/lib.rs").read_text(encoding="utf-8")

failures = []

step = re.search(
    r"fn execution_step\s*\([^)]*\)\s*->\s*Result<ExecutionStatusView,\s*String>\s*\{(.*?)\n\}",
    tauri,
    re.S,
)
if step:
    body = step.group(1)
    if "start_task" in body and "dispatch" not in body and "adapter" not in body:
        failures.append(
            "P7-IA-01: execution_step creates a task attempt/lease but does not dispatch through Antigravity"
        )
else:
    failures.append("P7-IA-01: could not locate execution_step production command")

if 'executable: "mock-adapter".into()' in execution or 'exit_code: Some(0)' in execution[
    execution.find("pub fn dispatch_with_adapter"):execution.find("pub fn snapshot_json")
]:
    failures.append(
        "P7-IA-07: production dispatch_with_adapter synthesizes a successful mock process exit"
    )

if "if trusted.signers.is_empty()" in execution:
    failures.append(
        "P7-IA-08: P7 admission checks only that the trusted signer set is non-empty, not that the sealed registry matches the verified trusted registry"
    )

if failures:
    print("P7 independent desktop/source static audit: FAIL")
    for failure in failures:
        print(" -", failure)
    sys.exit(1)

print("P7 independent desktop/source static audit: PASS")
