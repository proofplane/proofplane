# PASETO Token Migration Epic

Replace actor-owned API keys with self-service, user-owned PASETO tokens and
move attachment download grants to a separate encrypted PASETO profile. The
core principle is explicit authority: API access is attributable to a user and
token, while each token is constrained to one workspace and an immutable
permission set.

Full rationale, schema, token profiles, rollout, and decisions live in
[spec.md](./spec.md), the source of technical depth. Tickets below are lean
handoff units that link into it.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [PASETO Keyrings And Token Primitives](./tickets/001-paseto-keyrings-and-token-primitives.md) | Done | Add `pasetors`, separate key domains, configuration validation, and tested wrappers. |
| 002. [Self-Service User API Tokens](./tickets/002-self-service-user-api-tokens.md) | Done | Persist, issue, list, and revoke user-owned workspace tokens. |
| 003. [PASETO Data-Plane Authentication](./tickets/003-paseto-data-plane-authentication.md) | Todo | Build and test the shared `v4.public` authenticator and `ApiTokenContext`. |
| 004. [Evidence Attribution And Actor Retirement](./tickets/004-evidence-attribution-and-actor-retirement.md) | Todo | Atomically switch routes/provenance, remove actors, and consolidate the final schema into one `V001`. |
| 005. [PASETO Attachment Download Grants](./tickets/005-paseto-attachment-download-grants.md) | Todo | Issue encrypted `v4.local` grants while preserving current download safety checks. |

## Sequencing

- **001** is foundational for both token profiles and can ship without changing
  external behavior.
- **002** depends on 001 and establishes the token records and management API
  consumed by the verifier.
- **003** depends on 002 and builds the shared PASETO authenticator without
  changing external route behavior.
- **004** depends on 003 and atomically replaces the actor contract. There is no
  compatibility or deployed-data preservation phase; local databases are reset
  and rebuilt from the consolidated initial migration.
- **005** depends only on 001 and can proceed in parallel with 002-004. Because
  the download service is not deployed, it replaces JWT grants atomically
  without a compatibility phase.
