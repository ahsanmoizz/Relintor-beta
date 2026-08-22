from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[2]
evidence = (root / "crates/relintor-evidence/src/lib.rs").read_text(encoding="utf-8")
evidence_cargo = (root / "crates/relintor-evidence/Cargo.toml").read_text(encoding="utf-8")
desktop_cargo = (root / "apps/desktop/src-tauri/Cargo.toml").read_text(encoding="utf-8")
tauri = (root / "apps/desktop/src-tauri/src/lib.rs").read_text(encoding="utf-8")
backend = (root / "apps/desktop/src/backend.ts").read_text(encoding="utf-8")
app = (root / "apps/desktop/src/App.tsx").read_text(encoding="utf-8")

failures = []

if "relintor-evidence" not in desktop_cargo:
    failures.append("P8-IA-01: desktop does not depend on relintor-evidence")

if "relintor_evidence" not in tauri:
    failures.append("P8-IA-01: Tauri production source does not import/use P8 evidence authority")

if not re.search(r"verification_(start|status|run|rerun|evidence|certificate)", tauri, re.I):
    failures.append("P8-IA-01: no P8 verification command surface is wired into Tauri")

if "relintor-execution" not in evidence_cargo:
    failures.append("P8-IA-04: P8 crate cannot authenticate/consume the P7 ExecutionRun authority")

if re.search(
    r"pub\s+fn\s+put\s*\(\s*&self\s*,\s*mut\s+metadata\s*:\s*EvidenceMetadata\s*,\s*bytes",
    evidence,
    re.S,
):
    failures.append(
        "P8-IA-02: production EvidenceStore still accepts raw caller-created evidence metadata"
    )

if "ai_judgements: Vec::new()" in evidence:
    failures.append(
        "P8-IA-07: verification/manifest path still hard-codes AI judgement references empty"
    )

if ".filter(|criterion| criterion.machine_checkable)" in evidence:
    failures.append(
        "P8-IA-08: completion path still drops non-machine-checkable acceptance criteria"
    )

collector_names = [
    "ApiDatabaseCollector",
    "BrowserRuntimeCollector",
    "ScreenshotCollector",
    "AccessibilityCollector",
    "PerformanceCollector",
    "SecurityCollector",
]
missing_collectors = [name for name in collector_names if name not in evidence]
if missing_collectors:
    failures.append(
        "P8-IA-06: observation DTOs still lack production collector boundaries: "
        + ", ".join(missing_collectors)
    )

# A real production verifier adapter may keep live execution PENDING, but it
# must exist rather than being only a generic injected test trait/status string.
if "ProductionAiProvider" not in evidence and "GatewayAi" not in evidence:
    failures.append(
        "P8-IA-07: no concrete production AI-gateway verifier adapter exists"
    )

# Static safety signal: plain invalidation JSON without any integrity field/index.
inv_match = re.search(r"pub struct EvidenceInvalidation\s*\{(.*?)\}", evidence, re.S)
if inv_match and "integrity" not in inv_match.group(1).lower() and "signature" not in inv_match.group(1).lower():
    if "invalidation_index" not in evidence.lower() and "invalidation_chain" not in evidence.lower():
        failures.append(
            "P8-IA-05: invalidation authority is not authenticated / deletion-detectable"
        )

# Product UX should expose actual P8 actions, not only marketing copy containing
# the word evidence/verification.
product_text = backend + "\n" + app
if not re.search(r"(verificationStatus|verificationStart|runVerification|rerunVerification|completionCertificate)", product_text):
    failures.append("P8-IA-01: desktop frontend/backend has no actual P8 verification workflow")

if failures:
    print("P8 independent source/product audit: FAIL")
    for item in failures:
        print(" -", item)
    sys.exit(1)

print("P8 independent source/product audit: PASS")
