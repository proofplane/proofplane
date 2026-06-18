# 004 - Evidence Attribution And Actor Retirement

**Status:** Todo · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#attribution-and-actor-retirement)

**Summary** - Atomically replace actor authentication with PASETO, make
API-token identity the durable provenance for new evidence, and delete all
actor-era management surfaces, code, schema, seeds, and dependencies. Replace
the migration history with one final initial schema for fresh local databases.

**Acceptance criteria**

- [ ] Given a valid PASETO bearer token, when it calls an allowed data-plane route after cutover, then the request runs as that user and API token.
- [ ] Given a PASETO-authenticated evidence submission, when it is created, then only its API-token foreign key is stored; when it is read, then the submitting user is derived through that token.
- [ ] Given an empty database, when migrations run, then exactly one `V001` creates the complete final schema with API-token provenance and no actor or audit-event tables.
- [ ] Given a local database created by the old migrations, when this change is adopted, then the documented workflow resets and recreates it rather than attempting an upgrade or backfill.
- [ ] Given `x-proofplane-*` headers or actor management routes after cutover, when they are used, then they are rejected or absent with no compatibility path.
- [ ] Given an API token whose membership or permission is insufficient, when actor removal ships, then the existing 401/404 authorization guarantees remain unchanged.
- [ ] Given seed, worker, and repository operations, when actors are removed, then system work uses explicit system context and no fabricated actor identity.

**Tasks**

- [ ] Replace the migration history with one `V001__initial_schema.sql` containing the complete final schema; remove the old incremental migration files.
- [ ] Replace actor repository transaction/read contexts with user-token and explicit system contexts.
- [ ] Wire every REST data-plane route to the shared PASETO verifier and preserve permission/workspace non-disclosure behavior.
- [ ] Update evidence DTOs, fixtures, and audit fields to the API-token-derived `submitted_by` contract.
- [ ] Remove actor routes/services/domain types, authentication, headers, tracing fields, and repository code.
- [ ] Remove `api-keys-simplified` and replace actor-based local/integration seeds with users and PASETO tokens.
- [ ] Update local reset/bootstrap documentation for the fresh user/API-token schema.
- [ ] Add migration and integration tests for the single fresh schema, final authorization, new attribution, old-header rejection, and absence of actor surfaces.

**Notes**

- Revised with the 2026-06-17 spec update: evidence stores the API-token ID and
  derives its user rather than duplicating `user_id` on the submission.
- Revised again on 2026-06-17: no actor data or contract is preserved; this
  ticket performs the complete external replacement atomically.
- Revised again on 2026-06-17: schema history is consolidated into one `V001`;
  old local databases are reset rather than migrated.
