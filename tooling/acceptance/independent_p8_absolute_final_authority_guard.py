from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[2]
src = (root / "crates/relintor-evidence/src/lib.rs").read_text(encoding="utf-8")
failures = []

provider = re.search(
    r"pub\s+struct\s+AiProviderResult\s*\{(?P<body>[^}]*)\}", src, re.S
)
if not provider:
    failures.append("ZC-02: AiProviderResult production type is missing")
else:
    body = provider.group("body")
    if re.search(r"pub\s+(judgement|provider_identity|response_digest)\s*:", body):
        failures.append("ZC-02: AiProviderResult authority fields are public")
    derive = src[max(0, provider.start() - 180) : provider.start()]
    if "Serialize" in derive or "Deserialize" in derive:
        failures.append("ZC-02: AiProviderResult remains serde-constructible")

if "ProtectedCriterionVerificationPlan" not in src:
    failures.append("ZC-01: protected criterion verification authority is missing")
if "empty_for_authority" not in src:
    failures.append("ZC-01: production does not default to an empty protected plan")
if "mutable workspace candidate verification config" not in src:
    failures.append("ZC-01: workspace verification config is not clearly candidate-only")
if "candidate_command_digest" not in src:
    failures.append("ZC-01: candidate command digest is not checked at the binding boundary")

if failures:
    print("P8 absolute final authority guard: FAIL")
    for failure in failures:
        print(" -", failure)
    sys.exit(1)

print("P8 absolute final authority guard: PASS")
