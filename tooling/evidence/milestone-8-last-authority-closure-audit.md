# Relintor P8 — Last Authority/Product Closure Audit

## Verdict

`MILESTONE_8_LAST_AUTHORITY_REPAIR_REQUIRED`

This is not a broad P8 reopen. The P8-FC-01..09 repairs are materially present,
but the final certification delta exposes four remaining production-authority
defects.

P9 must remain NOT STARTED until P8-LC-01..04 are closed.

## P8-LC-01 — test-support is still a DEFAULT production feature

`crates/relintor-evidence/Cargo.toml` currently has:

`default = ["test-support"]`

The desktop explicitly disables default features, but any ordinary production
Rust dependency on `relintor-evidence` receives the evidence-minting test
support by default.

Repair:
- `default = []`
- keep `test-support = []` opt-in only;
- production crates must not enable it;
- acceptance tests may explicitly enable/use it as test support.

## P8-LC-02 — verification orchestration is hard-coded to Cargo and blocks every other evidence class

`VerificationCollectorOrchestrator::collect_required_evidence` maps:

- BuildOutput -> `cargo build --workspace --locked`
- TestOutput -> `cargo test --workspace --locked`
- LintStaticAnalysis -> Cargo Clippy
- every other class -> "live collector dependency is unavailable"

This makes the user-runnable verification path Rust/Cargo-specific and means
configured browser/API/database/security/accessibility/performance collectors
are never invoked by the desktop orchestrator.

Repair:
- consume a Rust-owned verification collector plan derived from sealed project
  authority + P4/P5 discovered project/runtime/tooling facts;
- select commands/adapters appropriate to the actual project (pnpm/npm/Gradle,
  Cargo, Python, etc.) rather than hard-code Cargo;
- route every applicable configured evidence class to its real collector;
- unavailable dependencies remain BLOCKED/PENDING, but a configured collector
  must be executable by the orchestrator;
- do not let renderer/LLM supply arbitrary commands as verification authority.

## P8-LC-03 — any successful obligation collector currently claims every acceptance criterion

`CollectorBinding::for_requirement` copies *all* acceptance criteria for a
requirement into `accepted_criteria`.

`VerificationEngine::evaluate` then considers a criterion satisfied when any
fresh PASS artifact lists that criterion.

Therefore, for a requirement with BuildOutput + runtime criteria, a successful
build receipt can claim the runtime criteria even though the collector never
tested them.

Repair:
- create a sealed/trusted criterion-to-evidence/probe mapping;
- a collector receipt may list only criteria that its specific collector/probe
  actually evaluated;
- BuildOutput cannot satisfy unrelated API/browser/security/runtime criteria;
- missing criterion-specific proof blocks VERIFIED_COMPLETE.

## P8-LC-04 — production AI verification is not integrated and maps any non-empty response to SUPPORTED

`ProductionAiProvider` now calls the gateway, but:
- every non-empty gateway response becomes `AiJudgementKind::Supported`;
- no structured SUPPORTED/UNSUPPORTED/INCONCLUSIVE/CONFLICT parsing exists;
- desktop `evaluate_p8` creates the engine with no AI judgements and never calls
  the production AI verifier;
- the acceptance test for deterministic-fail-vs-AI still evaluates the engine
  and AI provider separately rather than one integrated decision.

Repair:
- require structured gateway output and validate its schema;
- never interpret arbitrary non-empty text as SUPPORTED;
- when sealed policy requires independent AI review, the Rust-owned verification
  orchestrator calls the production provider, authenticates/persists the
  judgement reference, and passes it into the same VerificationEngine run;
- unavailable live provider remains PENDING/BLOCKED truthfully;
- in the same integrated path, deterministic FAIL must override AI SUPPORTED.

## Closure

P8 can be certified for continuation after:
- the included static closure audit passes;
- the included criterion-provenance Rust test passes;
- the prior P8 final closure 2/2 and source audit 3/3 remain green;
- the strengthened 43-test acceptance corpus remains green;
- workspace/desktop/native/security regression gates remain green.
