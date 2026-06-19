# 006 - Compact Opaque User API Tokens

**Status:** Todo · **Depends on:** 004 · **Spec:** [spec.md](../spec.md#user-api-tokens)

**Summary** - Replace long self-contained `v4.public` user credentials with
compact `ppat_` opaque tokens whose digests resolve the existing persisted
authority. Preserve ownership, workspace scoping, permissions, revocation, and
evidence attribution while removing the redundant API signing-key domain.

**Acceptance criteria**

- [ ] Given a current workspace member, when they issue an API token, then the raw value is shown once as a 41-character `ppat_` token with valid Base62 randomness and checksum while only its SHA-256 digest and lifecycle metadata are persisted.
- [ ] Given a valid active token, when it authenticates a data-plane request, then its digest resolves the owning user, workspace, token ID, and permissions into the existing `ApiTokenContext`.
- [ ] Given a malformed, bad-checksum, unknown-digest, superseded `v4.public`, revoked, expired, or stale-membership token, when authentication is attempted, then it returns 401 without leaking token existence or raw secret material.
- [ ] Given a valid token for another workspace or without the required permission, when it addresses a protected resource, then the existing 404 non-disclosure behavior is unchanged.
- [ ] Given token listing, idempotent revocation, or evidence attribution, when the opaque format ships, then existing user ownership, metadata, and API-token provenance contracts are unchanged.
- [ ] Given existing attachment download grants, when this ticket ships independently of ticket 005, then their JWT or `v4.local` behavior is unchanged.

**Tasks**

- [ ] Add a small Proofplane-owned opaque-token generator and parser with unbiased Base62 randomness, deterministic CRC32 checksum handling, and SHA-256 digesting.
- [ ] Add a unique non-null token digest to the consolidated `V001` schema and repository operations for digest lookup without storing raw tokens.
- [ ] Replace management issuance with opaque-token generation while preserving the response DTO, validation, lifecycle metadata, and one-time disclosure behavior.
- [ ] Replace `v4.public` verification with digest-backed authentication and persisted authority while preserving membership checks, authorization policy, and best-effort last-use updates.
- [ ] Remove API PASETO claims, signer/verifier, signing-key configuration, and tests while retaining the `v4.local` download key domain required by ticket 005.
- [ ] Reissue local seed credentials and update integration fixtures, API examples, and secret-redaction coverage for the `ppat_` format.
- [ ] Add unit, migration, and integration tests for generation, checksum and digest behavior, successful authentication, every rejection class, and unchanged authorization/provenance contracts.

**Notes**

- The 2026-06-19 spec revision supersedes the `v4.public` user-token decisions
  delivered by tickets 001-004; their actor retirement and persisted lifecycle
  work remains in force.
- The service is not deployed, so this is an atomic credential-format cutover
  with no PASETO compatibility or digest backfill.
