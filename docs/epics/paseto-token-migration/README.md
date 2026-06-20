# API Token And PASETO Migration Epic

Replace actor-owned API keys with compact, self-service user API tokens and move
attachment download grants to an encrypted PASETO profile. The core principle
is fit-for-purpose authority: human-managed API credentials are short opaque
references to persisted permissions, while short-lived download grants carry a
stateless encrypted payload.

Full rationale, schema, token profiles, rollout, and decisions live in
[spec.md](./spec.md), the source of technical depth. Tickets below are lean
handoff units that link into it.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [PASETO Keyrings And Token Primitives](./tickets/001-paseto-keyrings-and-token-primitives.md) | Done | Add `pasetors`, separate key domains, configuration validation, and tested wrappers. |
| 002. [Self-Service User API Tokens](./tickets/002-self-service-user-api-tokens.md) | Done | Persist, issue, list, and revoke user-owned workspace tokens. |
| 003. [PASETO Data-Plane Authentication](./tickets/003-paseto-data-plane-authentication.md) | Done | Built the shared API-token authentication context later backed by opaque-token digest lookup. |
| 004. [Evidence Attribution And Actor Retirement](./tickets/004-evidence-attribution-and-actor-retirement.md) | Done | Atomically switched routes/provenance, removed actors, and consolidated the final schema into one `V001`. |
| 005. [PASETO Attachment Download Grants](./tickets/005-paseto-attachment-download-grants.md) | Done | Issues encrypted `v4.local` grants while preserving current download safety checks. |
| 006. [Compact Opaque User API Tokens](./tickets/006-compact-opaque-user-api-tokens.md) | Done | Replaced long `v4.public` API credentials with `ppat_` opaque tokens resolved by an indexed digest. |

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
- **005** depends only on 001. Because the download service is not deployed, it
  replaces JWT grants atomically without a compatibility phase.
- **006** depends on the completed actor cutover in 004. It replaces the
  `v4.public` credential format atomically, retains the user-token lifecycle and
  attribution model, and removes the API signing-key domain.
- **005** and **006** can proceed in parallel, but changes to shared PASETO
  configuration and authentication modules require merge coordination. After
  both ship, `pasetors` remains only for attachment download grants.
