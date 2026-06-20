# 004 - Evidence Attribution And Actor Retirement

**Status:** Done · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#attribution-and-actor-retirement)

**Summary** - Atomically replace actor authentication with user API tokens, make
API-token identity the durable provenance for new evidence, and delete all
actor-era management surfaces, code, schema, seeds, and dependencies. Replace
the migration history with one final initial schema for fresh local databases.

**Acceptance criteria**

- [x] Given a valid opaque API bearer token, when it calls an allowed data-plane route after cutover, then the request runs as that user and API token.
- [x] Given an API-token-authenticated evidence submission, when it is created, then only its API-token foreign key is stored; when it is read, then the submitting user is derived through that token.
- [x] Given an empty database, when migrations run, then exactly one `V001` creates the complete final schema with API-token provenance and no actor or audit-event tables.
- [x] Given a local database created by the old migrations, when this change is adopted, then the documented workflow resets and recreates it rather than attempting an upgrade or backfill.
- [x] Given `x-proofplane-*` headers or actor management routes after cutover, when they are used, then they are rejected or absent with no compatibility path.
- [x] Given an API token whose membership or permission is insufficient, when actor removal ships, then the existing 401/404 authorization guarantees remain unchanged.
- [x] Given seed, worker, and repository operations, when actors are removed, then system work uses explicit system context and no fabricated actor identity.

**Tasks**

- [x] Replace the migration history with one `V001__initial_schema.sql` containing the complete final schema; remove the old incremental migration files.
- [x] Replace actor repository transaction/read contexts with user-token and explicit system contexts.
- [x] Wire every REST data-plane route to the shared API-token authenticator and preserve permission/workspace non-disclosure behavior.
- [x] Update evidence DTOs, fixtures, and audit fields to the API-token-derived `submitted_by` contract.
- [x] Remove actor routes/services/domain types, authentication, headers, tracing fields, and repository code.
- [x] Remove `api-keys-simplified` and replace actor-based local/integration seeds with users and opaque API tokens.
- [x] Update local reset/bootstrap documentation for the fresh user/API-token schema.
- [x] Add migration and integration tests for the single fresh schema, final authorization, new attribution, old-header rejection, and absence of actor surfaces.

**Notes**

- Revised with the 2026-06-17 spec update: evidence stores the API-token ID and
  derives its user rather than duplicating `user_id` on the submission.
- Revised again on 2026-06-17: no actor data or contract is preserved; this
  ticket performs the complete external replacement atomically.
- Revised again on 2026-06-17: schema history is consolidated into one `V001`;
  old local databases are reset rather than migrated.
- The 2026-06-19 spec revision changes only the user credential format in
  ticket 006; actor retirement, API-token provenance, route cutover, and the
  consolidated-schema decision remain in force.
