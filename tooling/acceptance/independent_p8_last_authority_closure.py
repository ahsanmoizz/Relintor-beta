from pathlib import Path
import re, sys

root = Path(__file__).resolve().parents[2]
ev = (root / "crates/relintor-evidence/src/lib.rs").read_text(encoding="utf-8")
cargo = (root / "crates/relintor-evidence/Cargo.toml").read_text(encoding="utf-8")
tauri = (root / "apps/desktop/src-tauri/src/lib.rs").read_text(encoding="utf-8")
acc = (root / "crates/relintor-evidence/tests/independent_p8_acceptance.rs").read_text(encoding="utf-8")

failures = []

if re.search(r'default\s*=\s*\[\s*"test-support"\s*\]', cargo):
    failures.append("P8-LC-01: test-support remains enabled by default")

start = ev.find("impl VerificationCollectorOrchestrator")
end = ev.find("\n}", start)
orchestrator = ev[start:start+16000] if start >= 0 else ""
if (
    '"cargo".into()' in orchestrator
    and '"build".into()' in orchestrator
    and '"test".into()' in orchestrator
    and '"clippy".into()' in orchestrator
):
    failures.append("P8-LC-02: product verification orchestrator remains Cargo-hard-coded")
if 'live collector dependency is unavailable' in orchestrator and not any(
    token in orchestrator
    for token in (
        "BrowserRuntimeCollector",
        "ApiDatabaseCollector",
        "SecurityCollector",
        "AccessibilityCollector",
        "PerformanceCollector",
        "ScreenshotCollector",
    )
):
    failures.append("P8-LC-02: non-Cargo evidence classes are never routed through configured collectors")

for_req_start = ev.find("pub fn for_requirement")
for_req = ev[for_req_start:for_req_start+7000] if for_req_start >= 0 else ""
if ".acceptance_criteria" in for_req and ".collect::<BTreeSet<_>>()" in for_req:
    failures.append("P8-LC-03: for_requirement still grants every requirement criterion to one collector binding")

provider_start = ev.find("impl IndependentAiProvider for ProductionAiProvider")
provider = ev[provider_start:provider_start+6500] if provider_start >= 0 else ""
if "if response.text.trim().is_empty()" in provider and "AiJudgementKind::Supported" in provider:
    failures.append("P8-LC-04: any non-empty AI gateway text is still interpreted as SUPPORTED")

eval_start = tauri.find("fn evaluate_p8")
eval_body = tauri[eval_start:eval_start+5000] if eval_start >= 0 else ""
if "with_ai_judgements" not in eval_body and "IndependentAiVerifier" not in eval_body:
    failures.append("P8-LC-04: desktop P8 evaluation does not integrate AI judgements when policy requires them")

# The mandatory precedence test must be integrated, not two disconnected assertions.
m = re.search(r"fn deterministic_failure_wins_over_ai_supported\s*\([^)]*\)\s*\{", acc)
if m:
    body = acc[m.start():m.start()+4500]
    if ".evaluate(" in body and ".verify(" in body and ".with_ai_judgements(" not in body:
        failures.append("P8-LC-04: deterministic-fail-vs-AI acceptance remains disconnected")

if failures:
    print("P8 last authority/product closure audit: FAIL")
    for f in failures:
        print(" -", f)
    sys.exit(1)

print("P8 last authority/product closure audit: PASS")
