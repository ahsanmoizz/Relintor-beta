# Relintor P8 — Final Certification Guard

## Verdict

`MILESTONE_8_FINAL_CERTIFICATION_REPAIR_REQUIRED`

This is not a new broad P8 audit. It is the promised review of the four
P8-LC repair paths in the final certification delta.

LC-01 is closed.
LC-02 routing is materially closed.
LC-04 structured gateway parsing + desktop integration is materially present.

Two authority defects remain inside LC-03/LC-04.

## P8-CC-01 — Same-class criteria are still over-claimed

`CollectorBinding::for_requirement` calls `criterion_ids_for_class`.

That function assigns every acceptance criterion whose `criterion_type` maps
to the evidence class.

Example:

- criterion A: "login rejects invalid password", type `functional`
- criterion B: "refund succeeds", type `functional`
- required evidence class: `TEST_OUTPUT`

One successful unrelated test-process receipt receives both criterion IDs.

That is still false criterion provenance. Class compatibility is not proof that
the specific probe evaluated the specific acceptance criterion.

Repair:
- a receipt may claim a criterion only from a trusted criterion/probe mapping;
- generic class matching can select candidate collectors, but must not mark
  every same-class criterion satisfied;
- if no criterion-specific mapping/probe exists, keep the criterion unverified;
- a test/build/runtime receipt should carry only criterion IDs actually covered
  by the executed probe/test inventory.

The included Rust test demonstrates this exact defect.

## P8-CC-02 — Raw caller-created AI judgements remain a public authority input

`VerificationEngine::with_ai_judgements(Vec<AiVerifierJudgement>)` is public and
accepts plain caller-created judgement structs.

The desktop path now obtains structured judgements from the production gateway,
but another production Rust caller can still construct:

`AiVerifierJudgement { kind: Supported, ... }`

and inject it into completion authority.

The method only validates basic shape; it does not prove the judgement came
from `ProductionAiProvider` / the server-side gateway.

Repair:
- production engine accepts an authenticated AI-verifier receipt/reference,
  not a raw `AiVerifierJudgement`;
- bind provider/gateway identity, requirement ID, minimized input digest,
  evidence IDs, response digest, timestamp and authority identity;
- authenticate it with the Rust-owned P8 authority;
- keep raw judgement injection test-only if needed;
- deterministic failure precedence remains unchanged.

## Closure rule

After these two repairs:
- included criterion-provenance test PASS;
- included static AI-authority check PASS;
- P8 last-authority 1/1 remains PASS;
- final product closure 2/2 remains PASS;
- source audit 3/3 remains PASS;
- P8 acceptance 43/43 remains PASS;
- workspace/desktop/native/security gates remain green.

Then P8 is eligible for `MILESTONE_8_CERTIFIED_FOR_CONTINUATION`.
