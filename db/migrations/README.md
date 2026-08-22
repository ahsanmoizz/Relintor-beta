# Migration boundary

## Local desktop SQLite

`001_baseline.sql` is the first local SQLite migration used by `relintor-core`.

## Cloud PostgreSQL

`002_cloud_account_foundation.sql` creates the initial account and entitlement
schema.

`003_milestone2_reconciliation.sql` is an additive independent-audit repair. It
does not rewrite migration 002. It adds:

- session access-token/organization fields;
- usage buckets;
- complimentary grants;
- admin mutation records.

The cloud runtime must execute PostgreSQL migrations against a real database and
must verify the recorded migration version/name. Merely parsing SQL text or
tracking versions in an in-memory set is not migration verification.
