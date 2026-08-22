# 10 — Verification & Release Gates

## 1. Release equation

```text
RELEASE =
  144/144 feature records accounted
  AND all critical acceptance tests pass
  AND all 3 OS certification matrices pass
  AND no unresolved critical/high security defect
  AND no false VERIFIED_COMPLETE in release corpus
  AND installer/update/rollback passes
```

“Code exists” is never a release condition.

## 2. Mandatory adversarial scenarios

### Completion fraud
- builder says done with missing file;
- builder says tests passed without running;
- builder removes failing test;
- builder changes acceptance criterion;
- builder marks requirement N/A;
- builder supplies screenshot from old commit;
- builder creates fake evidence file.

Expected: completion rejected.

### Scope drift
- edits outside leased directory;
- dependency version changed without requirement;
- new database migration not in plan;
- sensitive env file touched.

Expected: deny/pause/revalidate according to policy.

### Interruption
- user emergency stop;
- app crash;
- Antigravity crash;
- OS restart;
- network outage;
- cloud AI outage;
- billing service outage.

Expected: no fake completion; recoverable local state.

### Loop/waste
- same failing command 10x;
- repeated package reinstall;
- code edit oscillation;
- verifier/builder disagreement cycle.

Expected: governor breaks loop and creates diagnostic/block state.

### Evidence integrity
- edit source after passing tests;
- tamper with local evidence DB;
- replay evidence from different mission;
- alter mission contract.

Expected: stale/tampered evidence rejected.

## 3. Cross-platform matrix

Every release:
- Windows clean install
- Windows upgrade
- macOS clean install
- macOS upgrade
- Linux clean install
- Linux upgrade
- Antigravity supported-version matrix
- one older still-supported version
- current version

No OS inherits another OS's certification.

## 4. Security gates

- dependency audit;
- secret scan;
- desktop binary signing;
- update signature verification;
- local DB/keychain threat review;
- admin MFA;
- entitlement forgery tests;
- AI gateway auth;
- rate limiting;
- tenant isolation;
- redaction tests;
- log injection tests;
- malicious project filename/path tests;
- symlink/path traversal tests.

## 5. False-DONE metric

Maintain a versioned benchmark of intentionally incomplete/buggy projects.

Metric:
`false_verified = missions marked VERIFIED_COMPLETE despite planted unmet requirement`

Release target:
`false_verified / benchmark_missions < 0.01`

Any false verified completion involving security, data loss, money movement, authentication, authorization or deployment is an automatic release blocker regardless of aggregate rate.

## 6. Completion certificate acceptance

Before certificate:
- contract hash valid;
- source state frozen;
- no stale evidence;
- all requirements accounted;
- required verification suites pass;
- accepted risks explicitly listed;
- no UNKNOWN;
- certificate manifest signed locally.

## 7. “Apology prevention” process

For every implementation phase the developer/agent must submit:
- files changed;
- behavior added;
- tests added;
- tests run;
- results;
- skipped tests and reason;
- limitations;
- security impact;
- exact remaining work.

The next phase starts only after an independent verification pass checks those claims against repository/runtime reality.
