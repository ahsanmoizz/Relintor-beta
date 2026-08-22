# Schema boundary

The local SQLite schema is applied by the Rust-owned migration runner from
`db/migrations/001_baseline.sql`.

Milestone 1 contains only:

- `schema_migrations`
- `projects`
- `health_checks`

Later graph, evidence, execution, account, and entitlement tables must arrive
through explicit numbered migrations. Existing migration files must not be
silently edited after release.
