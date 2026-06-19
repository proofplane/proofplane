# PASETO Token Migration Spec

## Goal

Replace actor-owned opaque API credentials with user-owned personal API tokens,
and replace JWT attachment download grants with purpose-bound PASETO tokens.

The core authorization model becomes:

- Auth0 identifies a human on management routes.
- A workspace member creates API tokens for themselves.
- Each API token is bound to one user, one workspace, and an explicit subset of
  data-plane permissions.
- Data-plane requests authenticate with one bearer token and are attributed to
  both the user and the token.
- Attachment download grants remain short-lived bearer URLs and use a separate
  encrypted token profile.

This epic supersedes the actor and API-key portions of the Auth Hierarchy API
spec after the migration is complete. Workspace membership remains the source
of truth for whether a user may act in a workspace.

## Existing Baseline

Proofplane currently has two identity planes:

- Auth0 bearer JWTs produce `UserContext` for workspace, membership, actor, and
  credential management.
- `x-proofplane-actor-id` plus `x-proofplane-api-key` authenticate data-plane
  routes. The API key is an opaque secret with an Argon2 hash in
  `api_credentials`; authentication loads `actors.workspace_id` and
  `actor_permissions` into `ActorContext`.

Attachment download grants are stateless five-minute HS256 JWTs. The grant
claims are readable, and every redemption reloads the attachment and validates
its current lifecycle and object metadata.

Actors also provide persisted provenance. `evidence_submissions.submitted_by`
references `actors.id`, repository read/write contexts carry an actor ID, and
audit plans identify actors. Removing actors therefore requires an attribution
schema change, not only an authentication change. Because none of this data is
deployed, the final schema does not preserve actor-era records or contracts.

## Decisions

### Single Initial Migration

Proofplane has no deployed database, so this epic squashes the complete schema
into one `migrations/V001__initial_schema.sql`. Delete the existing incremental
migrations and rewrite `V001` to describe only the final schema:

- users and workspace memberships;
- user-owned API tokens and `WorkspacePermission` grants;
- evidence provenance through `submitted_by_api_token_id`;
- all current compliance, outbox, and attachment tables;
- no actors, actor permissions, actor credentials, or dormant `audit_events`.

There is no forward upgrade or data-backfill path from the current local schema.
Developers run the existing destructive `make reset-local` workflow, recreate
the local dependencies, apply the single migration to an empty database, and
seed fresh user/API-token data. Migration and integration tests likewise start
from an empty database and apply only the consolidated `V001`.

### Library And Protocols

Use `pasetors` 0.7.8 with its high-level claims APIs and PASETO version 4:

- API tokens use `v4.public`.
- Attachment download grants use `v4.local`.

Version 0.7.8 is the current release as of June 17, 2026 and requires Rust
1.88 or newer; Proofplane's current toolchain satisfies that requirement. The
crate has not undergone a third-party security audit, so Proofplane's wrapper
must stay small, pin the dependency deliberately, validate all custom claims,
and cover the official failure classes in tests.

Do not expose raw `pasetors` types outside the authentication module. Domain and
service code consume Proofplane-owned verified token types.

### Separate Cryptographic Domains

API tokens and download grants must never share keys or purpose strings.

| Profile | PASETO purpose | Key authority | Implicit assertion |
| --- | --- | --- | --- |
| API access | `v4.public` | API signing private key; verifier public-key ring | `proofplane:api-access:v1` |
| Attachment download | `v4.local` | Dedicated symmetric-key ring | `proofplane:attachment-download:v1` |

Each token carries a non-secret `kid` in its authenticated footer. Verification
may inspect the untrusted footer only to select a candidate key; no footer or
payload value is trusted until cryptographic verification succeeds.

Configuration provides:

- one active API signing key and a public verification-key ring;
- one active download-grant key and a decryption-key ring;
- stable key IDs controlled by operators.

The logical configuration lives under `paseto.api` and `paseto.download`. Keys
use encodings accepted by `pasetors`/PASERK rather than a Proofplane-specific
binary format.

Startup fails on malformed keys, a missing active key, duplicate key IDs, or an
active key absent from its verification/decryption ring. Private and symmetric
key material remains in redacted secret configuration. Public verification keys
may be distributed to another verifier without granting minting authority.

Rotation adds a new active key while retaining old verification keys. Old API
public keys remain configured until every token signed by them is expired or
revoked; a token with a far-future expiration may therefore keep its signing
key in the verification ring for a long time. Removing an old public key
explicitly invalidates every remaining token signed by it. Old download keys
need remain only for the maximum grant lifetime plus deployment skew.

## User API Tokens

### Claims

An API token has this logical payload:

```text
iss             configured public API origin
aud             proofplane-api
sub             user UUID
jti             API token UUID
iat             issuance time
nbf             issuance time
exp             required expiration
version         1
workspace_id    workspace UUID
permissions     array of WorkspacePermission strings
```

Registered claims are validated through `ClaimsValidationRules`. Proofplane
manually validates every custom claim, rejects unknown permission strings,
rejects duplicate permissions, and requires the token version it understands.
The `exp` claim is required and must be strictly in the future at issuance.
Proofplane imposes no maximum token lifetime: any parseable future timestamp is
accepted, including dates centuries in the future. Permissions are serialized
in canonical enum order and compared to persistence as a set.

Tokens are immutable. Changing workspace, expiration, or permissions means
issuing a replacement and revoking the old token.

### Persistence

Ticket 002 adds these user-owned token records in an incremental `V005`
migration. Ticket 004 later folds the final post-cutover schema into the
consolidated `V001`.

```text
api_tokens
- id UUID PRIMARY KEY                         -- same value as jti
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
domain enum and the exact database values/check constraint currently used by
`actor_permissions`. Do not introduce a second API-token permission enum.

The raw token is returned once and is never persisted or logged. The verified
`jti` resolves the lifecycle row; exact user, workspace, expiration, and
permission agreement between claims and the row prevents token metadata from
drifting independently.

Token rows are retained after revocation because historical evidence and audit
records may reference them. Revocation is idempotent; management APIs do not
hard-delete tokens. `last_used_at` is informational and may be updated at most
once per hour so authentication does not create a database write per request.

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

The raw `v4.public` token appears only in the successful create response under
the `api_token` field.

## Data-Plane Authentication And Authorization

Data-plane routes move to:

```text
Authorization: Bearer v4.public....
```

The `x-proofplane-actor-id` and `x-proofplane-api-key` contract is retired.
Management routes continue to interpret bearer credentials as Auth0 JWTs;
data-plane routes interpret them as Proofplane PASETO API tokens. No route
accepts the actor headers after the cutover, and there is no dual-authentication
or deprecation period.

Authentication proceeds in this order:

1. Require exactly one bearer token with the `v4.public` purpose.
2. Read the untrusted footer only to select a configured public key.
3. Verify the signature, registered claims, audience, issuer, implicit
   assertion, and custom claim structure.
4. Load `api_tokens` and `api_token_permissions` by `jti`.
5. Require matching user, workspace, expiration, and permission claims.
6. Reject a revoked or expired token.
7. Require that the user still has a membership in the token workspace.
8. Produce `ApiTokenContext`.

```text
ApiTokenContext
- user_id
- api_token_id
- workspace_id
- permissions
```

Invalid, unknown, revoked, expired, malformed, or stale-membership tokens return
401. After authentication, a path workspace mismatch or missing route
permission returns 404, preserving the existing tenant non-disclosure rule.
Requests using the old actor headers are unauthenticated and receive 401.

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

Preferred implementation sequence:

1. Add PASETO configuration and token primitives without changing traffic.
2. Add user token management and begin issuing `v4.public` tokens.
3. Build and test the PASETO data-plane authenticator behind a shared internal
   interface, but do not wire it to HTTP routes yet.
4. Atomically switch every data-plane route to PASETO, replace evidence
   provenance, remove actor headers/routes/code, and squash the schema into one
   final `V001`.
5. Replace attachment-grant JWT issuance and verification with `v4.local`, and
   remove the JWT helper and signing-secret configuration in the same change.

## Test Strategy

Unit coverage includes:

- `v4.public` and `v4.local` round trips;
- tampering, wrong key, unknown `kid`, wrong purpose/assertion, issuer,
  audience, version, malformed custom claims, future validity, and expiry;
- API-token claim/row matching and permission parsing;
- startup rejection for invalid key-ring configuration.

Integration coverage includes:

- self-service issue/list/revoke with raw token shown once;
- cross-user and cross-workspace management rejection;
- successful data access and each permission rejection;
- revoked, expired, membership-removed, and claim/row mismatch rejection;
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

## References

- [pasetors 0.7.8 documentation](https://docs.rs/pasetors/0.7.8/pasetors/)
- [PASETO version 4 specification](https://github.com/paseto-standard/paseto-spec/blob/master/docs/01-Protocol-Versions/Version4.md)
