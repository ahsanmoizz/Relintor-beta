# 04 — Investigation & Standards Engine

## 1. Purpose

Users cannot ask for engineering decisions they do not know exist. Relintor must expose latent requirements without turning every project into an enterprise architecture exercise.

## 2. New Project Investigator

Inputs:
- free-form idea;
- documents;
- sketches/screenshots;
- repo/template if supplied;
- desired deployment;
- user answers.

Outputs:
- product ontology;
- personas;
- critical user journeys;
- non-functional requirements;
- architecture decisions;
- risk register;
- applicable standards packs;
- requirement graph;
- evidence plan.

## 3. Question policy

Ask only questions whose answer changes:
- architecture;
- scope;
- data model;
- security;
- compliance exposure;
- SEO/accessibility;
- deployment;
- cost;
- verification.

Every question includes:
- why it matters;
- recommended default;
- consequence of each choice.

“No idea” is a valid answer.

## 4. Existing Project Takeover

Reality reconstruction sequence:

```text
repository inventory
→ build-system detection
→ dependency graph
→ route/API discovery
→ database/schema discovery
→ auth/permission discovery
→ UI journey discovery
→ tests/CI discovery
→ deployment discovery
→ docs/promises extraction
→ runtime probes
→ capability reconciliation
```

Then classify each discovered capability:
`WORKING / PARTIAL / BROKEN / MISSING / UNPROVEN / DEAD`

## 5. Standards Registry

Standards are data, not hard-coded prompts.

Each standard rule stores:
- domain pack;
- rule ID;
- title;
- source;
- version/date;
- applicability predicate;
- severity;
- rationale;
- expected implementation patterns;
- deterministic checks where possible;
- evidence requirements;
- exceptions.

## 6. Initial domain packs

Full Antigravity release includes curated packs for:

1. Web frontend
2. Backend/API
3. Databases
4. Authentication/authorization
5. Application security
6. Accessibility
7. SEO/discoverability
8. Performance
9. DevOps/release engineering
10. Observability/operations
11. Data/privacy
12. Payments/financial workflows
13. AI/ML applications
14. Blockchain/Web3
15. Mobile
16. Desktop
17. Data engineering
18. Third-party integrations

The engine applies only relevant rules.

## 7. Applicability example

Rule: public-site crawlability

```text
IF public_marketing_pages = true
AND organic_search_relevant = true
THEN require:
  crawlability decision
  metadata
  canonical URL policy
  sitemap/robots behavior
  structured-data applicability review
  performance baseline
ELSE mark N/A with reason
```

No “every app must use SSR” rule exists.

## 8. Standards updates

- signed by Relintor;
- versioned;
- diff shown internally;
- cannot retroactively change a sealed mission without revalidation;
- project may choose “evaluate against newer standards”;
- source/rationale must remain inspectable.

## 9. Architecture Decision Records

Every significant choice becomes an ADR with:
- context;
- options;
- selected decision;
- reason;
- tradeoffs;
- future trigger for reconsideration.

This is how the “foundation above current ground level” idea is implemented without speculative overengineering.
