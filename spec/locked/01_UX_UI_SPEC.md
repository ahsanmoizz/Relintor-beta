# 01 — UX/UI Specification

## 1. UX thesis

The engine is complicated. The interface must feel like a toy.

A new user should understand the product from four verbs:

**Describe → Seal → Watch → Verify**

The main navigation has only:

1. **Home**
2. **Projects**
3. **Activity**
4. **Account**

Team administration appears inside Account for authorized users. Technical internals remain hidden behind an optional “Details” drawer.

## 2. Visual system

Use the founder-supplied warm parchment/brown palette as the canonical design tokens.

### Typography
- Display / editorial: `Lora`
- UI / body: `Libre Baskerville`
- Machine evidence / ASCII / IDs: `IBM Plex Mono`

### Character
- paper-like background;
- thin brown rules;
- slightly squared 4px radius;
- subtle offset shadows;
- no glassmorphism;
- no neon gradients;
- no “AI purple”;
- no giant dashboard of meaningless metrics;
- status conveyed by words + icons, never color alone.

## 3. Core shell

Desktop shell:

```text
┌─────────────────────────────────────────────────────────────────────┐
│ RELINTOR                                      ● Guardian active     │
├────────────┬────────────────────────────────────────────────────────┤
│ Home       │                                                        │
│ Projects   │                   CURRENT VIEW                         │
│ Activity   │                                                        │
│ Account    │                                                        │
│            │                                                        │
│            │                                                        │
│ v2.x       │                                                        │
└────────────┴────────────────────────────────────────────────────────┘
```

## 4. Home

Only three important actions:

```text
What are you building?

[ + New project ]   [ Take over existing project ]

Recent missions
────────────────────────────────────
Acme Portal        Building     63%
Ledger App         Verified     ✓
Old SaaS           Needs review !
```

No technical setup controls on Home.

## 5. New Project flow

### Step A — Describe
One large text box and drag/drop documents.

Prompt:
> Tell me what you want to exist when this is finished.

Optional:
- attach docs;
- choose folder;
- choose Git repository.

### Step B — Investigator
Conversation-like, but structured. Every question shows **why it matters**.

Example:

```text
Will public pages depend on Google search traffic?

Why I'm asking:
This changes rendering, metadata and crawlability requirements.

( ) Yes
( ) No
( ) Not sure
```

The user can choose “Not sure”; Relintor must propose a recommendation and tradeoff.

### Step C — Blueprint
The user sees plain-language cards:
- Product
- Users
- Architecture
- Data
- Security
- SEO/accessibility/performance
- Deployment
- Testing
- Future assumptions
- Risks
- Explicitly deferred decisions

Each card has `Why`, `Decision`, `Evidence required`.

### Step D — Seal
One final page:

```text
Mission scope      38 requirements
Quality gates      91 checks
External actions    4 need explicit authority
Estimated phases   11

[ Seal & Build ]

After sealing, scope changes require revalidation.
Emergency stop remains available.
```

## 6. Existing Project Takeover flow

1. Select project folder.
2. Relintor inventories repository.
3. Show “What I found” before asking questions.
4. Reconstruct product capabilities.
5. Compare docs/promises to runtime/code.
6. Ask only unresolved founder questions.
7. Generate keep/repair/remove/rebuild decisions.
8. Seal takeover mission.

Takeover summary uses these labels only:

`WORKING` · `PARTIAL` · `BROKEN` · `MISSING` · `UNPROVEN` · `DEAD/UNUSED`

## 7. Mission Cockpit — the primary product screen

```text
┌────────────────────────────────────────────────────────────────────┐
│ Acme Portal                                      [Emergency stop]  │
│ Building · Phase 6/11 · Guardian active                           │
├──────────────────────────────┬─────────────────────────────────────┤
│ CURRENT                      │ PROOF                               │
│ Authentication hardening    │ 17/22 checks verified              │
│                              │                                     │
│ Antigravity                  │ ✓ build                             │
│ Editing 3 files              │ ✓ unit tests                        │
│                              │ ✓ runtime login                      │
│  █████████████░░  72%        │ … expiry test running               │
│                              │                                     │
│ Next                         │ Open evidence →                     │
│ Verify reset-token expiry    │                                     │
├──────────────────────────────┴─────────────────────────────────────┤
│ Requirements  26/38 verified   Blocked 1   Failed 0   Unknown 2   │
└────────────────────────────────────────────────────────────────────┘
```

The cockpit is not a chat transcript. It is a mission state view.

## 8. Evidence view

The user clicks a requirement and sees:

```text
R-014  Password reset
Status: VERIFIED

Implementation
✓ API route found
✓ UI route found
✓ token persistence found

Tests
✓ valid token
✓ invalid token
✓ expired token
✓ reuse rejected

Runtime
✓ browser flow
✓ database change
✓ mail adapter result

Evidence bundle
E-1042 … E-1051
```

## 9. Failure UX

Never show:
> Something went wrong.

Show:
- what failed;
- what Relintor tried;
- whether repository state changed;
- current safe checkpoint;
- what can happen next.

Buttons:
`Retry safely` · `Change decision` · `Open details` · `Stop mission`

## 10. Emergency stop UX

Single button, always visible during execution.

Sequence:
`Stopping safely → checkpointing → reconciling files → recording incomplete requirements`

Final state:
`STOPPED_INCOMPLETE`

Resume button appears only after integrity/revalidation checks.

## 11. Completion UX

A verified mission ends with a calm certificate, not confetti:

```text
VERIFIED COMPLETE

38 / 38 requirements accounted for
38 verified
0 failed
0 blocked
0 unknown
0 silently deferred

Build: PASS
Runtime: PASS
Security baseline: PASS
Release checks: PASS

[ View certificate ] [ Export evidence ]
```

## 12. Account

- profile
- plan
- AI usage included by plan
- devices
- billing
- privacy
- team/company
- sign out

No API-key screen exists.

## 13. Accessibility
- WCAG 2.2 AA target
- full keyboard navigation
- visible focus
- screen-reader labels
- reduced motion
- 200% zoom support
- status not encoded only by color
- high contrast tested for light/dark themes

## 14. UX acceptance

A first-time user must be able to:
1. create/take over a project;
2. answer investigation;
3. seal;
4. watch progress;
5. understand a failure;
6. stop safely;
7. resume;
8. inspect why a feature is considered verified;

without opening a terminal or reading product documentation.
