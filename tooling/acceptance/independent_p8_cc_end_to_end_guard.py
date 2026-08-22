from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[2]
src = (root / "crates/relintor-evidence/src/lib.rs").read_text(encoding="utf-8")

failures = []

orch_start = src.find("impl VerificationCollectorOrchestrator")
orch = src[orch_start:orch_start+25000] if orch_start >= 0 else ""
if "CollectorBinding::for_requirement" in orch and "CollectorBinding::for_criterion" not in orch:
    failures.append(
        "P8-CC-E2E-01: production orchestrator never creates criterion-specific collector bindings"
    )

eval_start = src.find("pub fn evaluate(")
eval_body = src[eval_start:eval_start+30000] if eval_start >= 0 else ""
if (
    "human_or_runtime_satisfied = !criterion.machine_checkable" in eval_body
    and ".accepted_criteria" not in eval_body[
        eval_body.find("human_or_runtime_satisfied"):
        eval_body.find("let ai_satisfied", eval_body.find("human_or_runtime_satisfied"))
    ]
):
    failures.append(
        "P8-CC-E2E-01: unrelated runtime/human PASS evidence can satisfy a non-machine criterion"
    )

if re.search(r"pub\s+struct\s+AiProviderResult\s*\{[^}]*pub\s+judgement\s*:", src, re.S):
    if re.search(r"pub\s+fn\s+authenticate_ai_judgement\s*\(", src):
        failures.append(
            "P8-CC-E2E-02: caller can fabricate public AiProviderResult and ask the engine to authenticate it"
        )

if failures:
    print("P8 CC end-to-end closure guard: FAIL")
    for failure in failures:
        print(" -", failure)
    sys.exit(1)

print("P8 CC end-to-end closure guard: PASS")
