# 00 — Product Constitution

## 1. Mission

Relintor exists because AI coding systems can produce believable activity without producing a complete, professional, verified result.

The product owns three failures:

1. **False completion** — an agent claims completion without sufficient evidence.
2. **Latent engineering ignorance** — users do not know which professional decisions, standards, tests, and future-facing foundations they should ask for.
3. **Execution abandonment or corruption** — long software missions are interrupted, loop, drift, waste usage, or stop with partially changed state.

## 2. Immutable promise

> Give Relintor an idea, specification, or existing project. Relintor determines what professional completion requires, seals the mission, governs the Antigravity execution loop, continuously accounts for requirements and evidence, and does not issue a verified completion result until the evidence supports it.

## 3. The authority hierarchy

From strongest to weakest:

1. deterministic safety and verification results;
2. sealed project contract;
3. explicit founder/user decisions captured before sealing;
4. curated professional standards registry;
5. independent verifier judgement;
6. builder-agent claims.

A lower layer may never override a deterministic failure.

## 4. Completion states

Only these final states exist:

- `VERIFIED_COMPLETE`
- `COMPLETE_WITH_ACCEPTED_RISKS`
- `STOPPED_INCOMPLETE`
- `BLOCKED_EXTERNAL`
- `FAILED_VERIFICATION`
- `REVALIDATION_REQUIRED`

There is no generic green “Done” state.

## 5. Requirement accounting invariant

Every sealed requirement must always be exactly one of:

- `UNSTARTED`
- `IN_PROGRESS`
- `IMPLEMENTED_UNVERIFIED`
- `VERIFIED`
- `FAILED`
- `BLOCKED`
- `NOT_APPLICABLE`
- `DEFERRED_BY_EXPLICIT_DECISION`

A requirement may never disappear from the graph because the agent forgot it.

## 6. Founder decisions frozen

- Closed-source commercial software.
- Monthly and annual subscriptions.
- Individual and team customers.
- Founder/admin can create complimentary access, trials, custom entitlements and company grants.
- User never configures API keys for Relintor.
- Relintor pays for its own AI intelligence usage.
- Local-first project state.
- Source code is not bulk-uploaded to Relintor servers by default.
- Cloud AI receives only the minimum scoped context needed for a requested judgement, after local secret scanning/redaction; retention policy must be zero/limited according to provider contract.
- Windows, macOS and Linux ship together for the Antigravity release.
- Strict sealed execution is the default.
- Emergency stop always remains available.
- Normal users use GUI only; terminal/PowerShell/manual JSON is not required.
- Antigravity is the first execution adapter, not a hard-coded permanent dependency.
- Product quality beats preserving any existing stack.

## 7. What “99%” means

Do not market “99% bug-free software.”

Internal quality target:

> **False verified-completion rate < 1% on the maintained acceptance corpus.**

Additionally:

> **100% of sealed requirements must be accounted for.**

## 8. Product boundaries

### Included in the first full release
- New Project mode
- Existing Project Takeover mode
- Professional standards intelligence
- project questioning/investigation
- sealed architecture and requirement contract
- task decomposition
- Antigravity execution governance
- permission automation
- execution watchdog
- anti-loop detection
- requirement traceability
- deterministic and AI-assisted verification
- browser/runtime verification
- recovery and resume
- evidence explorer
- completion certificate
- user/team subscriptions
- founder admin
- cross-platform installers and updates

### Explicitly excluded from this release
- Claude/Codex/Cursor execution adapters
- claiming universal OS interception outside routed missions
- irreversible external-side-effect rollback where no compensation exists
- legal/compliance certification guarantees
- “all code is correct” guarantees
- Windows 7 support

## 9. Product ethics and truth rule

Relintor must hold itself to the same standard it imposes on agents:
- no capability is listed as complete without test evidence;
- skipped acceptance tests are visible;
- unsupported platforms are not marketed;
- inferred evidence is labeled inferred;
- user-accepted risk is not silently converted to verified success.
