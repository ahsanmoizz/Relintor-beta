from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[2]
src = (root / "crates/relintor-evidence/src/lib.rs").read_text(encoding="utf-8")

failures = []

# Production must not accept raw, caller-created AI judgement structs as an
# authority-bearing input.
if re.search(
    r"pub\s+fn\s+with_ai_judgements\s*\(\s*mut\s+self\s*,\s*ai_judgements\s*:\s*Vec<AiVerifierJudgement>",
    src,
    re.S,
):
    failures.append(
        "P8-CC-02: public VerificationEngine::with_ai_judgements still accepts raw caller-created AI authority"
    )

if failures:
    print("P8 final certification guard: FAIL")
    for failure in failures:
        print(" -", failure)
    sys.exit(1)

print("P8 final certification guard: PASS")
