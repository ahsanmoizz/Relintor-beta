# Relintor P8 — CC End-to-End Closure Guard

## Verdict

`MILESTONE_8_CC_END_TO_END_REPAIR_REQUIRED`

This is not new P8 scope. It is a source review of only P8-CC-01 and P8-CC-02
from the final four-file delta.

## P8-CC-E2E-01 — criterion provenance is not wired end-to-end

The generic binding is now safe: `CollectorBinding::for_requirement` claims no
criteria, and `for_criterion` can represent one exact criterion.

However the production orchestrator never calls `for_criterion`; it only calls
`for_requirement`. Therefore production collector runs do not mint
criterion-specific proof.

Additionally, `VerificationEngine::evaluate` still contains a non-machine
shortcut where any fresh PASS HumanDecision/BrowserRecording/ApiResponse/
DatabaseQuery artifact for the requirement can satisfy a non-machine criterion
without checking `accepted_criteria`.

Repair:
- the verification plan/orchestrator must execute criterion-specific probes and
  create `for_criterion` bindings when a criterion is actually covered;
- bind the real collector identity and executed probe/test identity;
- generic obligation evidence must not prove a criterion;
- runtime/human evidence must also explicitly carry the criterion ID it proved;
- AI may satisfy criteria only through its authenticated requirement review
  policy, not through unrelated runtime evidence.

## P8-CC-E2E-02 — AI authentication can still authenticate caller-fabricated provider results

`AiProviderResult` is a public struct with public fields and
`VerificationEngine::authenticate_ai_judgement` is public.

A production Rust caller can construct its own `AiProviderResult` containing a
SUPPORTED judgement and ask the engine to HMAC-authenticate it. That bypasses
the intended ProductionAiProvider/gateway provenance.

Repair:
- production callers must not be able to construct the provider result that is
  eligible for authentication;
- prefer an opaque provider receipt/result with private fields that can only be
  produced by the real provider adapter;
- or expose a high-level engine method that itself calls ProductionAiProvider
  and authenticates the returned response internally;
- keep raw/provider-result injection test-only;
- desktop remains provider -> structured parser -> authenticated receipt ->
  VerificationEngine.

P9 remains NOT STARTED until these exact two paths are closed.
