from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[2]
src = (root / "crates/relintor-execution/src/lib.rs").read_text(encoding="utf-8")

failures = []

def body(marker: str) -> str:
    start = src.find(marker)
    if start < 0:
        return ""
    brace = src.find("{", start)
    if brace < 0:
        return ""
    depth = 0
    for i in range(brace, len(src)):
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            depth -= 1
            if depth == 0:
                return src[start:i+1]
    return src[start:]

orchestrator = body("pub fn execute_next_with_adapter")
if not orchestrator:
    failures.append("P7-FC-02: execute_next_with_adapter missing")
else:
    auth = orchestrator.find("authorize_action")
    dispatch = orchestrator.find("dispatch_with_adapter")
    if auth < 0 or dispatch < 0 or auth > dispatch:
        failures.append(
            "P7-FC-02: adapter dispatch is not scheduler-authorized before mutable execution"
        )
    if "result.ended_at_ms" not in orchestrator:
        failures.append(
            "P7-FC-02: adapter result end time is not used for post-dispatch lease/completion authority"
        )

if re.search(r"pub\s+fn\s+from_p6_handoff\s*\(", src):
    failures.append(
        "P7-FC-04: public registry-unbound from_p6_handoff constructor still exists"
    )

external = body("pub fn detect_external_modification")
if external and ".starts_with(expected)" in external:
    failures.append(
        "P7-FC-03: external modification classification still uses lexical expected-path prefix matching"
    )

if failures:
    print("P7 final source closure audit: FAIL")
    for failure in failures:
        print(" -", failure)
    sys.exit(1)

print("P7 final source closure audit: PASS")
