# 03 — Antigravity Integration Contract

## 1. Integration principle

Antigravity is an execution engine. Relintor is mission authority.

Never automate the Antigravity GUI by mouse/keystroke as the primary integration.

Use:
- CLI/headless execution;
- plugin packaging;
- lifecycle hooks;
- permissions;
- SDK only where its support matrix is suitable;
- structured logs/transcripts;
- project/worktree boundaries.

## 2. Components

### A. Relintor Antigravity Adapter
Rust interface:

```text
detect()
install_bridge()
validate_version()
create_execution()
send_task()
stream_events()
request_stop()
collect_artifacts()
reconcile_exit()
```

### B. Relintor Plugin
Contains:
- rules;
- skills;
- hooks;
- optional MCP bridge;
- constrained subagent definitions where useful.

### C. Headless Runner
Each task execution is a supervised child process.

Captures:
- process ID;
- start/end;
- exit status;
- stdout/stderr;
- transcript/artifact references;
- token/credit telemetry when exposed;
- working directory;
- environment fingerprint.

## 3. Why the Stop hook is not completion authority

Antigravity intentionally limits repeated stop-hook continuations so a bad hook cannot deadlock a conversation.

Therefore:

```text
Agent wants to finish
→ Stop hook reports state to Relintor
→ if turn may continue, request continuation
→ if platform ends turn anyway:
    Relintor independently evaluates mission graph
    if incomplete, start the next controlled execution turn
```

Mission continuity lives outside the Antigravity process.

## 4. Tool interception

Pre-tool policy:

```text
expected + in scope + safe        → ALLOW
expected + protected authority    → ALLOW WITH LEASE
ambiguous                         → DENY + corrective instruction
outside sealed scope              → DENY
destructive but approved contract → ALLOW exact payload only
new dangerous authority           → HUMAN EXCEPTION REQUIRED
```

The user does not click ordinary allow prompts. Relintor makes them from the sealed contract.

## 5. Exact action binding

An execution grant binds:
- mission ID;
- task ID;
- canonical tool name;
- canonical arguments;
- workspace;
- environment;
- expiry;
- policy version;
- authority scope.

Changed payload = new evaluation.

## 6. Task packet sent to Antigravity

Every execution turn receives:
- task objective;
- exact allowed files/scope where known;
- relevant requirements;
- architecture decisions;
- professional constraints;
- required verification evidence;
- previous failure evidence;
- forbidden changes;
- stop condition.

No giant “build the whole app” prompt.

## 7. Builder/verifier separation

Builder:
- may edit within lease;
- may run allowed tools;
- may propose completion;
- cannot set requirement VERIFIED.

Verifier:
- separate Relintor verification job;
- read-only by default;
- cannot silently repair while judging;
- deterministic gates run first;
- AI review can downgrade confidence but cannot override deterministic failure.

## 8. Anti-loop rules

Watch for:
- same failing command repeated;
- edit/revert oscillation;
- repeated identical tool payload;
- no requirement progress across N actions;
- growing token/credit consumption without evidence gain;
- repeated dependency install attempts;
- repeated test execution without code/evidence change.

Actions:
1. warn execution planner internally;
2. inject diagnostic task;
3. switch strategy/model where allowed;
4. checkpoint;
5. terminate turn;
6. resume with clean context;
7. mark BLOCKED if bounded retries fail.

## 9. Compatibility gate

Every supported Antigravity version gets:
- install detection;
- plugin load test;
- hook load test;
- tool deny test;
- tool allow test;
- headless execution test;
- stop/restart continuity test;
- transcript collection test;
- sandbox/permission test;
- browser verification test where applicable.

A version failing the compatibility gate is shown as “Update required” or “Temporarily unsupported,” never silently used.
