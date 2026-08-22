from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[2]
ev = (root / "crates/relintor-evidence/src/lib.rs").read_text(encoding="utf-8")
ev_cargo = (root / "crates/relintor-evidence/Cargo.toml").read_text(encoding="utf-8")
tauri = (root / "apps/desktop/src-tauri/src/lib.rs").read_text(encoding="utf-8")
acceptance = (root / "crates/relintor-evidence/tests/independent_p8_acceptance.rs").read_text(
    encoding="utf-8"
)

failures = []

# FC-01: verification product path must actually collect applicable evidence.
eval_start = tauri.find("fn evaluate_p8")
eval_end = tauri.find("fn verification_view", eval_start)
eval_body = tauri[eval_start:eval_end] if eval_start >= 0 and eval_end > eval_start else ""
if not (
    "VerificationCollectorOrchestrator" in eval_body
    or "collect_required_evidence" in eval_body
    or "run_required_collectors" in eval_body
):
    failures.append(
        "P8-FC-01: desktop evaluate_p8 evaluates the store but does not run a verification collector orchestrator"
    )

# FC-02: doc-hidden is not test-only.
if re.search(r"pub\s+fn\s+test_fixture\s*\(", ev):
    failures.append("P8-FC-02: CollectorReceipt::test_fixture remains public in production")
if re.search(r"pub\s+fn\s+put_test_fixture\s*\(", ev):
    failures.append("P8-FC-02: EvidenceStore::put_test_fixture remains public in production")

# FC-03: report auth must be keyed, not public sha256.
report_impl = ev[ev.find("impl VerificationReport"):ev.find("pub struct AcceptedRiskRecord")]
if "sha256(&self.signing_body()" in report_impl:
    failures.append("P8-FC-03: VerificationReport integrity remains an unkeyed SHA-256 checksum")

# FC-04: weak authority constructors should not remain public production API.
if re.search(r"pub\s+fn\s+new\s*\(\s*store:\s*EvidenceStore", ev):
    failures.append("P8-FC-04: public VerificationEngine::new still permits no-P7 authority")
completion_impl = ev[ev.find("impl CompletionAuthority"):ev.find("impl CompletionCertificate")]
if re.search(r"pub\s+fn\s+issue\s*\(\s*&self\s*,\s*report:", completion_impl, re.S):
    failures.append("P8-FC-04: public CompletionAuthority::issue still permits no-P7 issuance")

# FC-06: explicit production collector surfaces for H-02..H-04.
for name in ("BuildCollector", "TestCollector", "StaticAnalysisCollector"):
    if name not in ev:
        failures.append(f"P8-FC-06: {name} production collector boundary is missing")

# Shallow observation construction in the mandated acceptance corpus.
for name in (
    "ApiDatabaseObservation",
    "BrowserRuntimeObservation",
    "AccessibilityObservation",
    "PerformanceObservation",
    "SecurityObservation",
):
    if re.search(rf"let\s+observation\s*=\s*{name}\s*\{{", acceptance):
        failures.append(
            f"P8-FC-06: acceptance still directly constructs {name} instead of executing its collector"
        )

# FC-07: current "production" provider is a fail-only stub and is not wired to
# the existing gateway crate.
if "relintor-ai-gateway" not in ev_cargo:
    failures.append("P8-FC-07: relintor-evidence is not wired to the existing relintor-ai-gateway")
provider_start = ev.find("impl IndependentAiProvider for ProductionAiProvider")
provider_end = ev.find("impl<P: IndependentAiProvider>", provider_start)
provider_body = ev[provider_start:provider_end] if provider_start >= 0 else ""
if provider_body and ".judge(" not in provider_body.replace("fn judge", "") and "Err(format!" in provider_body:
    failures.append("P8-FC-07: ProductionAiProvider remains a configured-but-always-Err stub")

# FC-08: raw authority key must not remain the production desktop key path.
if "load_or_create_secure_local_authority_key" in tauri:
    failures.append("P8-FC-08: desktop still loads P8 certificate/evidence authority from a raw app-data key file")
if 'authority_root.join("p8-local-authority.key")' in ev:
    failures.append("P8-FC-08: raw p8-local-authority.key storage remains in production evidence authority")

# FC-09: a no-op test does not prove N/A/defer semantics.
if "assert_eq!(\n        RequirementStatus::NotApplicable,\n        RequirementStatus::NotApplicable" in acceptance:
    failures.append("P8-FC-09: N/A/defer acceptance test is still a no-op enum equality test")

if failures:
    print("P8 final product/authority closure audit: FAIL")
    for item in failures:
        print(" -", item)
    sys.exit(1)

print("P8 final product/authority closure audit: PASS")
