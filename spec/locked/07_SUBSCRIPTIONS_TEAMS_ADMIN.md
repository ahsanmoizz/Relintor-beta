# 07 — Subscriptions, Teams & Founder Admin

## 1. Commercial model

Closed-source SaaS-assisted desktop subscription.

Plans are entitlement bundles, not hard-coded UI forks.

Suggested initial structure:
- Individual
- Pro
- Team
- Enterprise

Monthly + annual billing.

Exact prices are intentionally not sealed until real AI/provider/support cost data exists.

## 2. Account model

Entities:
- User
- Device
- Organization
- Membership
- Role
- Subscription
- Entitlement
- UsageBucket
- ComplimentaryGrant
- InvoiceReference
- AdminAction

## 3. Entitlements

Examples:
- max active projects
- monthly Relintor AI allowance
- verification depth
- team seats
- shared policy
- evidence retention
- remote sync
- admin/audit exports
- priority models
- enterprise identity features

## 4. No user API keys

Clients authenticate to Relintor's AI gateway with signed user/device/session credentials.

Provider credentials remain server-side.

Usage enforcement:
`plan allowance → soft warning → hard budget rule / upgrade path`

Mission execution must never be corrupted by silently cutting an AI call mid-transaction. Budget exhaustion occurs at safe scheduling boundaries.

## 5. Founder/admin panel

Founder role can:
- search users/orgs;
- inspect plan/entitlements;
- grant own account unlimited internal entitlement;
- grant free individual access;
- grant free company access;
- choose expiration or permanent grant;
- create trial;
- extend trial;
- override usage limits;
- add/remove seats;
- suspend/re-enable account;
- view billing status;
- view aggregate usage/cost;
- issue account credit where billing provider supports it;
- see application/version adoption;
- revoke compromised devices;
- force minimum supported version;
- publish signed standards pack;
- stage product rollout;
- view service health;
- view admin audit trail.

Every admin mutation is logged with actor, reason, old value and new value.

## 6. Early-company program

`ComplimentaryGrant` fields:
- organization;
- reason;
- granted by;
- start;
- expiry nullable;
- plan template;
- seat limit;
- AI budget override;
- notes.

No payment method required when the entitlement is fully complimentary.

## 7. Offline grace

Desktop caches a signed entitlement lease.

Recommended behavior:
- normal online refresh;
- 7-day offline grace for paid/complimentary users;
- clear countdown only when refresh actually fails;
- after grace: existing evidence remains readable/exportable, new sealed executions are disabled until entitlement refresh.

Never lock users out of their own local project evidence.

## 8. Billing provider

Use an internal billing adapter. Stripe can be the first implementation if the company's merchant setup supports it. Provider choice must not leak into project/core code.
