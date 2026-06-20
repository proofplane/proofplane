# API Token And PASETO Migration Spec

## Goal

Replace actor-owned API credentials with user-owned personal API tokens, and
replace JWT attachment download grants with purpose-bound PASETO tokens. Human-
managed API tokens use a compact opaque format; PASETO is reserved for
short-lived attachment grants whose stateless encrypted payload is useful.

The core authorization model becomes:

- Auth0 identifies a human on management routes.
- A workspace member creates API tokens for themselves.
- Each API token is bound to one user, one workspace, and an explicit subset of
  data-plane permissions.
- Data-plane requests authenticate with one bearer token and are attributed to
  both the user and the token.
- Attachment download grants remain short-lived bearer URLs and use an
  encrypted PASETO profile.

This epic supersedes the actor and API-key portions of the Auth Hierarchy API
spec after the migration is complete. Workspace membership remains the source
of truth for whether a user may act in a workspace.

## Existing Baseline

Tickets 001-004 retired actors and established the current user-owned API-token
model. Auth0 bearer JWTs produce `UserContext` for management routes. Data-plane
routes accept user-owned `v4.public` PASETO bearer tokens and produce
`ApiTokenContext`; evidence provenance references the persisted API-token row.

The PASETO contains user, token, workspace, expiration, and permission claims,
but authentication still loads the lifecycle row and current workspace
membership from PostgreSQL. The self-contained claims and signature therefore
add substantial credential length and key-rotation complexity without removing
a persistence lookup.

Attachment download grants remain stateless five-minute HS256 JWTs. Their
claims are readable, and every redemption reloads the attachment and validates
its current lifecycle and object metadata. Ticket 005 replaces only these
grants with encrypted `v4.local` PASETO.

## Decisions

### Single Initial Migration

Proofplane has no deployed database, so this epic squashes the complete schema
into one `migrations/V001__initial_schema.sql`. Delete the existing incremental
migrations and rewrite `V001` to describe only the final schema:

- users and workspace memberships;
- user-owned API-token metadata, token digests, and `WorkspacePermission`
  grants;
- evidence provenance through `submitted_by_api_token_id`;
- all current compliance, outbox, and attachment tables;
- no actors, actor permissions, actor credentials, or dormant `audit_events`.

There is no forward upgrade or data-backfill path from the current local schema.
Developers run the existing destructive `make reset-local` workflow, recreate
the local dependencies, apply the single migration to an empty database, and
seed fresh user/API-token data. Migration and integration tests likewise start
from an empty database and apply only the consolidated `V001`.

### Library And Protocols

User API tokens are opaque bearer credentials resolved through PostgreSQL. They
do not use PASETO, JWT, or another self-contained claims format.

Use `pasetors` 0.7.8 with its high-level claims APIs and PASETO version 4 only
for attachment download grants, which use `v4.local`.

Version 0.7.8 is the current release as of June 17, 2026 and requires Rust
1.88 or newer; Proofplane's current toolchain satisfies that requirement. The
crate has not undergone a third-party security audit, so Proofplane's wrapper
must stay small, pin the dependency deliberately, validate all custom claims,
and cover the official failure classes in tests.

Do not expose raw `pasetors` types outside the authentication module. Download
services consume Proofplane-owned verified grant types.

### PASETO Download Key Domain

Attachment downloads use a dedicated symmetric-key ring and the implicit
assertion `proofplane:attachment-download:v1`. Opaque user API tokens have no
signing key and share no key material with download grants.

Each download grant carries a non-secret `kid` in its authenticated footer.
Verification may inspect the untrusted footer only to select a candidate key;
no footer or payload value is trusted until cryptographic verification
succeeds.

Configuration provides one active download-grant key, a decryption-key ring,
and stable operator-controlled key IDs under `paseto.download`. Ticket 006
removes `paseto.api` and its signing and verification keys. Download keys use
encodings accepted by `pasetors`/PASERK rather than a Proofplane-specific binary
format.

Startup fails on malformed keys, a missing active key, duplicate key IDs, or an
active key absent from its decryption ring. Symmetric key material remains in
redacted secret configuration.

Rotation adds a new active download key while retaining old decryption keys for
the maximum grant lifetime plus deployment skew. API-token rotation is a
per-token issue-and-revoke operation and has no operator signing-key lifecycle.

## User API Tokens

### Format

The complete user-facing token format is:

```text
ppat_<30 random Base62 characters><6 Base62 checksum characters>
```

The fixed length is 41 ASCII characters and the accepted shape is
`^ppat_[0-9A-Za-z]{36}$`. The underscore and alphanumeric body allow common
mouse-selection behavior to select the complete credential as one word. The
`ppat_` prefix identifies Proofplane personal API tokens to humans and secret
scanners.

The random portion is generated from a cryptographically secure random source
with unbiased Base62 sampling and supplies approximately 178 bits of entropy.
Base62 uses the ordered alphabet
`0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz`. The final six
characters are the zero-padded Base62 encoding of CRC-32/ISO-HDLC, as provided
by zlib-compatible CRC32 implementations, over the ASCII `ppat_` prefix and
random portion. The checksum supports typo and offline secret-format detection;
it is not an authenticator and is never used instead of the stored digest
lookup.

The token carries no user, workspace, permission, expiration, or key identifier
claims. All authority comes from the persisted lifecycle row and its permission
records.

Expiration remains required and must be strictly in the future at issuance.
Proofplane imposes no maximum token lifetime: any parseable future timestamp is
accepted, including dates centuries in the future.

Tokens are immutable. Changing workspace, expiration, or permissions means
issuing a replacement and revoking the old token.

### Persistence

Tickets 002 and 004 established these user-owned records and consolidated them
into `V001`. Ticket 006 adds `digest` directly to that undeployed initial
schema; there is no incremental migration or digest backfill.

```text
api_tokens
- id UUID PRIMARY KEY
- digest BYTEA NOT NULL UNIQUE                -- SHA-256 of the complete token
- user_id UUID NOT NULL REFERENCES users(id)
- workspace_id UUID NOT NULL REFERENCES workspaces(id)
- name TEXT NOT NULL
- expires_at TIMESTAMPTZ NOT NULL
- revoked_at TIMESTAMPTZ
- last_used_at TIMESTAMPTZ
- created_at TIMESTAMPTZ NOT NULL

api_token_permissions
- api_token_id UUID NOT NULL REFERENCES api_tokens(id) ON DELETE CASCADE
- permission TEXT NOT NULL                    -- WorkspacePermission value
- PRIMARY KEY (api_token_id, permission)
```

`api_token_permissions.permission` reuses the existing `WorkspacePermission`
domain enum and database values. Do not introduce a second API-token permission
enum.

The raw token is returned once and is never persisted or logged. Proofplane
stores the 32-byte SHA-256 digest of the complete token because the input has
high cryptographic entropy and does not require password-hardening. The unique
digest index resolves the lifecycle row directly. Token generation retries the
entire random value on the practically impossible unique-digest collision.

Token rows are retained after revocation because historical evidence and audit
records may reference them. Revocation is idempotent; management APIs do not
hard-delete tokens. `last_used_at` is informational; authentication attempts to
set it to the current database timestamp after every successful authentication.
This write is best-effort and is not part of authorization correctness.

### Management API

Auth0-authenticated users manage only their own tokens:

```text
POST   /workspaces/{workspace_id}/api-tokens
GET    /workspaces/{workspace_id}/api-tokens
DELETE /workspaces/{workspace_id}/api-tokens/{token_id}
```

Issuance requires current workspace membership and accepts a name, expiration,
and explicit permission list. Expiration must be present and in the future, but
has no maximum distance from issuance. Listing omits the raw token. Revocation
requires that the token belongs to both the current user and path workspace.
Unknown workspaces, non-membership, and cross-user/cross-workspace token IDs
return 404 to avoid existence leaks.

The raw opaque token appears only in the successful create response under the
existing `api_token` field.

## Data-Plane Authentication And Authorization

Data-plane routes move to:

```text
Authorization: Bearer ppat_....
```

The `x-proofplane-actor-id` and `x-proofplane-api-key` contract is retired.
Management routes continue to interpret bearer credentials as Auth0 JWTs;
data-plane routes interpret them as Proofplane opaque API tokens. No route
accepts the actor headers after the cutover. Because the service is not
deployed, ticket 006 atomically stops accepting `v4.public` API tokens and
reissues seed credentials; there is no dual-authentication or migration period.

Authentication proceeds in this order:

1. Require exactly one bearer token matching the fixed `ppat_` shape.
2. Validate the checksum and reject malformed tokens before persistence access.
3. Compute SHA-256 over the complete presented token.
4. Load `api_tokens` and `api_token_permissions` by the unique digest.
5. Reject an unknown, revoked, or expired lifecycle row.
6. Require that the owning user still has membership in the stored workspace.
7. Best-effort update `last_used_at`.
8. Produce `ApiTokenContext` entirely from persisted authority.

```text
ApiTokenContext
- user_id
- api_token_id
- workspace_id
- permissions
```

Invalid-checksum, unknown, revoked, expired, malformed, or stale-membership
tokens return 401. After authentication, a path workspace mismatch or missing
route permission returns 404, preserving the existing tenant non-disclosure
rule. Requests using the old actor headers are unauthenticated and receive 401.

## Attribution And Actor Retirement

### Evidence Provenance

New evidence submissions record the API token used to create them:

```text
evidence_submissions
- submitted_by_api_token_id UUID NOT NULL REFERENCES api_tokens(id)
```

There is no legacy actor-provenance column or backfill. The consolidated initial
schema creates `submitted_by_api_token_id` directly; old local submissions and
objects disappear when developers reset the local environment. Seed data is
then recreated under a user-owned API token.

`api_tokens` rows are retained after revocation, so the token foreign key
remains durable provenance.

Do not duplicate `user_id` on `evidence_submissions`. Each API token belongs to
exactly one user, so queries and response mapping derive the submitting user by
joining `evidence_submissions -> api_tokens -> users`.

Submission responses replace the ambiguous scalar `submitted_by` with:

```json
{
  "submitted_by": {
    "api_token_id": "uuid",
    "user_id": "uuid"
  }
}
```

This is an intentional contract change. Fixtures, REST, future MCP DTOs, and
audit fields must use the same attribution semantics.

### Repository Contexts

Request-scoped repository operations move from actor contexts to a context that
carries workspace, user, and API-token identity. Seed and worker operations use
an explicit system context rather than a fabricated system actor.

In the atomic cutover:

- remove actor management routes and services;
- remove `Actor`, `ActorId`, `ActorKind`, `ActorContext`, and actor repository
  contexts;
- remove `actors`, `actor_permissions`, and `api_credentials`;
- remove `api-keys-simplified`;
- remove the legacy `x-proofplane-*` headers;
- replace actor-based seed and integration fixtures with users and API tokens.

## Attachment Download Grants

Attachment grants use `v4.local` because clients do not need to inspect claims
and the same API trust boundary mints and redeems them. The dedicated symmetric
key both encrypts and authenticates this payload:

```text
iss                       configured public API origin
aud                       proofplane-attachment-download
jti                       grant UUID
iat / nbf / exp           five-minute lifetime
version                   2
workspace_id              workspace UUID
submission_id             evidence submission UUID
attachment_id             evidence attachment UUID
issued_by_user_id         user UUID
issued_via_api_token_id   API token UUID
```

The route remains:

```text
/attachment-downloads?token=v4.local....
```

The URL remains a bearer secret. Encryption hides identifiers from casual URL
inspection but does not make logging, browser history, referrers, analytics, or
link previews safe. Existing redaction, HTTPS, `private, no-store`, and
`no-referrer` requirements remain.

Grant issuance and redemption remain stateless. Redemption still reloads the
attachment, requires an eligible upload status and tenant relationship, and
validates object metadata before streaming.

The service is not deployed, so there is no JWT compatibility window. The
migration replaces JWT issuance and verification atomically, removes the legacy
signing-secret configuration, and accepts only `v4.local` grants. The endpoint
returns the same not-found response for every invalid token.

## Audit And Secret Handling

Authentication and audit logs may contain user ID, API token ID, workspace ID,
matched route, permission, and coarse outcome. They must never contain:

- raw API tokens or download grants;
- authorization headers or URL query strings;
- private, symmetric, or wrapped key material;
- decrypted claim payloads as an unbounded serialized value.

Evidence and MCP audit plans replace actor attribution with user and API-token
attribution. Metrics remain low-cardinality and must not use user, token, or
workspace IDs as labels.

## Build Sequence

Tickets 001-004 are complete and record the actor-to-user-token cutover. The
remaining preferred implementation sequence is:

1. Replace user `v4.public` issuance and authentication with compact opaque
   credentials, add digest persistence to `V001`, remove API signing-key
   configuration and primitives, and reissue local seed tokens atomically.
2. Replace attachment-grant JWT issuance and verification with `v4.local`, and
   remove the JWT helper and signing-secret configuration in the same change.

The two remaining tickets can proceed in parallel: the opaque-token pivot owns
user API authentication, while the download ticket retains only the `v4.local`
PASETO primitives and configuration. Changes to the shared authentication
module and configuration require merge coordination.

## Test Strategy

Unit coverage includes:

- exact opaque-token shape, unbiased random generation, deterministic checksum,
  malformed token rejection, digest stability, and digest non-disclosure;
- API-token lifecycle, expiration, membership, and permission parsing from the
  persisted row;
- `v4.local` round trips, tampering, wrong key, unknown `kid`, wrong
  purpose/assertion, issuer, audience, version, malformed custom claims, future
  validity, and expiry;
- startup rejection for invalid download key-ring configuration.

Integration coverage includes:

- self-service issue/list/revoke with raw token shown once;
- cross-user and cross-workspace management rejection;
- successful data access and each permission rejection;
- malformed, bad-checksum, unknown-digest, revoked, expired, and
  membership-removed rejection;
- rejection of superseded `v4.public` API tokens and removal of API signing-key
  configuration;
- an empty database migrated by the single `V001`, with no actor tables and
  API-token provenance plus derived user attribution;
- PASETO download grant issuance/redemption, hidden claim payload, tampering,
  expiry, wrong key, JWT rejection, and current attachment eligibility checks.

`make check` remains the completion gate. Docker-backed migration tests apply
the single `V001` to an empty database and route tests cover the final
authentication contract.

## Revisions

- 2026-06-17: Initial spec. Selected `pasetors`, `v4.public` user API tokens,
  `v4.local` attachment grants, staged actor retirement, and explicit
  user-plus-token evidence attribution.
- 2026-06-17: Made API-token expiration optional, reused `WorkspacePermission`
  for token grants, normalized evidence provenance to one API-token foreign key,
  and removed undeployed JWT/PASETO download compatibility.
- 2026-06-17: Restored required API-token expiration but removed any maximum
  lifetime; every parseable future `exp` is accepted.
- 2026-06-17: Removed actor-contract and actor-provenance compatibility. The
  pre-deployment cutover now deletes actor data and replaces the external
  contract atomically.
- 2026-06-17: Replaced the final actor-retirement cutover with one consolidated
  `V001`; local databases and storage are reset instead of upgraded.
- 2026-06-18: Clarified that ticket 002 still ships as incremental `V005`, with
  final `V001` consolidation deferred to ticket 004.
- 2026-06-19: Made API-token `last_used_at` updates best-effort on every
  successful authentication instead of hourly-throttled.
- 2026-06-19: Pivoted user API credentials from self-contained `v4.public`
  PASETO to 41-character `ppat_` opaque tokens with approximately 178 bits of
  entropy, an offline CRC32 checksum, and indexed SHA-256 digest persistence.
  Proofplane already requires lifecycle and membership lookups, so the PASETO
  claims duplicated persisted authority while making human-managed credentials
  cumbersome. Retained `v4.local` exclusively for short-lived encrypted
  attachment download grants.

## References

- [pasetors 0.7.8 documentation](https://docs.rs/pasetors/0.7.8/pasetors/)
- [PASETO version 4 specification](https://github.com/paseto-standard/paseto-spec/blob/master/docs/01-Protocol-Versions/Version4.md)
- [Behind GitHub's new authentication token formats](https://github.blog/engineering/behind-githubs-new-authentication-token-formats/)
- [GitLab `TokenAuthenticatable` storage strategies](https://docs.gitlab.com/development/token_authenticatable/)
- [OAuth 2.0 access-token formats](https://www.rfc-editor.org/rfc/rfc6749#section-1.4)
