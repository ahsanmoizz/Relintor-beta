# Milestone 2 Independent Audit — 2026-08-14

## Verdict

`MILESTONE_2_REPAIR_REQUIRED`

The submitted Windows test ledger is real evidence that the code compiled and
its current unit tests passed. It does **not** prove the sealed Milestone 2
contract was implemented.

## Release/continuation blockers found

### 1. PostgreSQL was not the runtime cloud state

The submitted cloud service required a PostgreSQL-looking URL but never opened a
PostgreSQL connection. Runtime accounts, organizations, sessions, devices and
plans lived in `AccountStore`, an in-memory map.

`/readyz` returned:

`{"status":"ready","database":"postgresql"}`

without checking a database.

The submitted "migration repeatability" test used an in-memory set of migration
version numbers and a string-marker test. It did not execute PostgreSQL SQL.

This conflicts with the Milestone 2 implementation contract requiring
PostgreSQL cloud state and migration verification.

### 2. Sealed P2 usage-accounting and founder-entitlement gates were missing

The sealed phase explicitly builds usage accounting and verifies:

- founder free entitlement works;
- AI budget counted.

The submitted gateway had no usage meter/budget enforcement. The database had no
UsageBucket/ComplimentaryGrant/AdminAction models from the sealed commercial
account model. A-11, L-06 and L-09 remained `not_started`.

### 3. Provider timeout was not a timeout

The submitted gateway called the provider synchronously and checked elapsed time
only after the provider returned. A hung provider could therefore hang the
request indefinitely.

### 4. Runtime provider selection could silently return mock output

`run_from_env` loaded credentials when a non-mock provider name was configured,
but still instantiated `MockProvider`. That could make a supposedly real
provider configuration return fake deterministic responses.

The repair fails closed until a real provider adapter exists.

### 5. AI gateway bearer authentication accepted any bearer value

The HTTP boundary checked only that `Authorization` started with `Bearer `.
There was no token validation. The repair uses an explicit development
authorization validator until account-session service integration exists.

### 6. AI gateway request IDs were response-header injectable

The submitted AI gateway echoed `x-request-id` into an HTTP response header
without the character/length validation already present in the cloud API.

### 7. HTTP request size enforcement stopped after header parsing

Both raw TCP HTTP parsers enforced the 1 MiB limit while finding headers, but
did not enforce it while reading a Content-Length body. A large declared body
could grow memory beyond the intended boundary.

### 8. Offline grace was controlled by the verifier caller, not the signature

The signed document contained `expires_at`, while `verify_entitlement` accepted
a caller-supplied `grace_seconds`. A client could therefore request an arbitrary
grace duration from the verification function.

The repair signs `grace_until`.

### 9. Tenant/issuer verification was incomplete

The signed entitlement verifier checked subject but did not enforce the expected
organization or issuer. The repair verifies both after signature validation.

### 10. Development access tokens were predictable

The submitted in-memory development tokens were deterministic SHA-256-derived
IDs based on account/sequence inputs. The repair uses operating-system-random
UUID v4 material for opaque development tokens.

## Repair contents

This repair pack:

- adds an additive migration 003 rather than rewriting migration 002;
- makes the runtime cloud repository PostgreSQL-backed;
- makes readiness check the real PostgreSQL connection;
- adds a mandatory live PostgreSQL integration gate;
- adds signed grace/tenant/issuer entitlement verification;
- adds usage budget accounting;
- adds complimentary-grant schema foundation;
- makes mock/real provider selection fail closed;
- implements a real response timeout boundary;
- validates development bearer credentials;
- hardens request IDs and HTTP body limits;
- repairs stale desktop phase wording;
- resets affected feature verification statuses to `not_run` until rerun.

## Important certification rule

A normal `cargo test --workspace --locked` is not enough to close this repair.

Milestone 2 cannot return a locally verified verdict until this command is
actually executed against a disposable **real PostgreSQL database**:

```text
cargo test -p relintor-cloud-api mandatory_live_postgres_migration_and_account_round_trip --locked -- --ignored --nocapture
```

with:

```text
RELINTOR_TEST_DATABASE_URL=<disposable local PostgreSQL database URL>
```

If PostgreSQL cannot be run, the truthful verdict remains:

`MILESTONE_2_IMPLEMENTED_UNVERIFIED`

The repair source itself is not evidence that the repair compiled or passed.
