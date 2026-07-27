# Auditor Auth0 Passwordless Spec

## Goal

Delegate auditor email-code generation, delivery, verification, expiry, replay
protection, and authentication attack controls to Auth0 Passwordless Email.
Proofplane continues to own the invitation secret, grant scope, review period,
revocation, audit trail, and seven-day portal session.

The core boundary is:

- Auth0 proves control of the invited email address.
- Proofplane decides what that authenticated identity may access.

This replaces the custom auditor OTP implementation and supersedes the
`auditor-otp-hmac` epic. It does not turn an auditor into a management-plane
Proofplane user or an Auth0 Organization member.

## Existing Baseline

An auditor currently presents a high-entropy invitation token, asks Proofplane
to send a six-digit code through Resend, submits that code to Proofplane, and
receives a digest-only local session cookie. Proofplane stores OTP state,
enforces send and verification limits, and owns the complete email-code
authentication path.

Management-plane identities already use Auth0, but the existing
`UserAuthenticator` JIT-provisions every accepted subject into `users`. Auditor
authentication must not reuse that provisioning path.

## Identity And Authorization Boundaries

Create a dedicated Auth0 Regular Web Application for the auditor portal and
enable only its intended passwordless email connection. Keep its client,
callback, and logout configuration separate from the management-plane and MCP
OAuth clients.

An authenticated auditor identity contains:

```rust
struct VerifiedAuditorIdentity {
    subject: String,
    email: String,
    email_verified: bool,
}
```

The auditor verifier never creates or updates a `users` row. A successful Auth0
callback creates an `AuditorSession` only after Proofplane verifies that:

- the authentication transaction was created from a valid invitation token;
- the transaction is unexpired and unused;
- the Auth0 response satisfies the OIDC contract below;
- `email_verified` is true;
- the normalized Auth0 email exactly matches the grant's `auditor_email`; and
- the grant remains active, unexpired, and unrevoked.

Auth0 `sub` is the stable authenticated principal. Email remains part of the
binding because the grant is explicitly issued to one mailbox. Use the same
normalization contract as grant creation and never authorize by email alone.

## Auth0 And Configuration Contract

Extend the existing Auth0 configuration with a dedicated auditor client:

```yaml
auth0:
  auditor_portal:
    client_id: "<regular-web-application client ID>"
    client_secret: "<secret-managed client secret>"
    callback_path: "/auditor-access/auth0/callback"
    connection: "email"
```

The existing issuer, JWKS URL, and public API base URL determine the Auth0
authorization endpoint, token endpoint, token verifier, and absolute callback
URL. Configuration validation requires non-blank values, an absolute callback
derived from the configured public base URL, and a callback path that cannot
escape the auditor-access namespace. Secrets and authorization codes are
redacted from debug and error output.

Production Auth0 configuration must provide:

- the exact callback URL on the application's allowlist;
- a Proofplane custom login domain where available;
- Passwordless Email enabled only for the auditor application;
- a production SMTP provider rather than Auth0's test provider;
- branded Universal Login and passwordless email templates;
- Auth0 brute-force and suspicious-IP protection enabled; and
- OTP length and expiry chosen together and recorded in the runbook.

Proofplane does not depend on Auth0 Organizations for this flow. Organization
membership is durable tenant membership and does not represent a temporary,
period-scoped auditor grant.

## Authentication Transaction

Add a short-lived, one-use transaction table:

```sql
CREATE TABLE auditor_auth_transactions (
    id UUID PRIMARY KEY,
    grant_id UUID NOT NULL REFERENCES auditor_access_grants(id) ON DELETE CASCADE,
    state_digest BYTEA NOT NULL UNIQUE,
    nonce_digest BYTEA NOT NULL,
    pkce_verifier TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (octet_length(state_digest) = 32),
    CHECK (octet_length(nonce_digest) = 32),
    CHECK (char_length(pkce_verifier) BETWEEN 43 AND 128)
);
```

Generate state, nonce, and PKCE verifier with the operating system CSPRNG.
Persist only SHA-256 digests of state and nonce. The PKCE verifier must be
available for the token exchange and is persisted only for the transaction's
short lifetime; never log or expose it.

Transactions expire after ten minutes. Creating a new transaction does not
make an older transaction valid for a different grant or email. The callback
atomically claims a matching unexpired, unconsumed transaction before making
the token exchange. A failed or interrupted callback consumes that transaction;
the auditor restarts from the still-valid invitation.

A periodic cleanup is unnecessary initially because callback and lookup
queries exclude expired rows. Delete expired and consumed rows opportunistically
when creating a new transaction for the same grant.

## Authorization Start

Replace the browser OTP request with:

```text
POST /auditor-access/{workspace_id}/login
```

The request carries the existing invitation token. Proofplane validates the
token and active grant before creating an authentication transaction. It then
redirects to Auth0 `/authorize` using Authorization Code Flow with PKCE S256:

- `client_id` from the dedicated auditor client;
- exact allowlisted `redirect_uri`;
- `response_type=code`;
- `scope=openid email`;
- random `state` and `nonce`;
- `code_challenge` and `code_challenge_method=S256`;
- `connection=email`;
- `login_hint` set to the grant email; and
- `prompt=login`.

`login_hint` improves the hosted experience but is untrusted and is not an
authorization control. `prompt=login` requires a fresh Auth0 authentication
when no Proofplane auditor session exists instead of silently accepting an
unrelated tenant SSO session.

Direct visits to Auth0 can initiate authentication but cannot create a
Proofplane session without a valid, server-created transaction and matching
grant. Auth0 tenant rate limits and attack protection remain necessary to limit
abuse of its public authentication surface.

## Callback And OIDC Verification

Handle the callback at:

```text
GET /auditor-access/auth0/callback
```

The callback:

1. Requires non-empty `code` and `state`.
2. Hashes `state` and atomically claims the matching transaction.
3. Exchanges the code at Auth0 using the client secret, exact redirect URI, and
   recorded PKCE verifier.
4. Verifies the ID token signature and requires RS256, the configured issuer,
   auditor client audience, valid `iat` and `exp`, non-empty `sub`, matching
   nonce digest, non-empty email, and `email_verified = true`.
5. Reloads the active grant and compares the normalized token email with the
   grant email.
6. Creates a local auditor session and redirects to the portal.

Define an auditor-specific Auth0 adapter boundary so tests can exercise token
exchange and identity outcomes without network access. The production adapter
uses the existing JWKS infrastructure but has a separate claims policy and
audience. It must distinguish rejected identity responses from temporary Auth0
or JWKS unavailability.

All callback failures render one coarse restart message. They never reveal
whether state, code, email, grant, or token validation caused the rejection.

## Auditor Sessions

Keep the existing random, digest-only auditor session cookie, seven-day
lifetime, grant revocation checks, last-used tracking, and portal authorization.
Add the Auth0 subject to newly authenticated sessions:

```sql
ALTER TABLE auditor_sessions
    ADD COLUMN auth0_subject TEXT
    CHECK (auth0_subject IS NULL OR btrim(auth0_subject) <> '');
```

The nullable migration preserves sessions issued by the old OTP flow during
their remaining lifetime. New session creation requires a non-blank Auth0
subject. After the maximum legacy session lifetime has elapsed, a later cleanup
may make the column required if useful.

Explicit logout revokes the Proofplane auditor session as it does today. Do not
invoke tenant-wide Auth0 logout because that could disrupt a management-plane
session in the same browser. `prompt=login` provides fresh auditor
authentication on the next portal login.

## Routes And Compatibility

Preserve:

- the invitation URL and high-entropy invitation-token format;
- the read-only portal paths and authorization behavior;
- the auditor session cookie name, attributes, and seven-day lifetime;
- grant expiry, revocation, review-period scope, and audit provenance; and
- active legacy auditor sessions during rollout.

Replace:

- `POST /auditor-access/{workspace_id}/otp/request/browser` with the login start;
- `POST /auditor-access/{workspace_id}/otp/verify/browser` with the Auth0
  callback; and
- Proofplane-rendered code and resend screens with branded Auth0 Universal
  Login.

Remove the JSON OTP request and verification endpoints. There is no non-browser
compatibility shim because accepting a Proofplane-verified OTP would preserve
the authentication path this epic is removing. This is an intentional contract
change and must be called out in release notes.

## Failure And Observability Contract

Use coarse browser outcomes:

- invalid or inactive invitation: existing unavailable response;
- rejected callback: restart from the invitation;
- Auth0 or JWKS unavailable: retryable service-unavailable response;
- session persistence failure: internal unavailable response.

Audit successful transitions with stable events such as:

- `auditor_access_auth.started`;
- `auditor_access_auth.completed`; and
- `auditor_access_session.created`.

Operational logs may include request ID, coarse failure category, transaction
ID, grant ID, and Auth0 subject only after successful verification. Never log
the invitation token, state, authorization code, ID token, client secret,
nonce, PKCE verifier, or session token. Do not log a submitted or token email
on failed authentication.

Auth0 authentication logs remain provider-side operational evidence. They do
not replace Proofplane's grant and session audit events.

## Migration And Cutover

Use additive migrations because `V001` may already be applied:

1. Add `auditor_auth_transactions` and nullable `auditor_sessions.auth0_subject`
   while leaving the OTP schema and routes operational.
2. Configure and validate the Auth0 auditor application in each environment.
3. Deploy the Auth0 start and callback flow and switch the invitation page.
4. Confirm all API instances use the new flow.
5. After at least ten minutes, remove the obsolete OTP routes, services,
   repository methods, table, mail adapter, Resend configuration, and tests in
   a separate cleanup change.

In-flight OTPs may be invalidated at the UI cutover and can be restarted from
their still-valid invitation. Existing local auditor sessions remain valid and
revocable. Rollback before cleanup can restore the old UI; rollback after the
OTP schema is removed requires redeploying the Auth0 flow rather than reviving
custom OTP verification.

## Test Contract

- Validate every new configuration field and secret-redaction path.
- Prove authorization URLs contain exact callback, PKCE S256, state, nonce,
  connection, login hint, scope, and fresh-login parameters.
- Prove state and nonce are random, stored only as digests, expire, and can be
  consumed only once.
- Prove rejected, replayed, expired, and cross-grant states never reach session
  creation.
- Prove code exchange uses the recorded PKCE verifier and exact callback URI.
- Prove invalid signature, algorithm, issuer, audience, lifetime, nonce,
  subject, email, and `email_verified` claims fail closed.
- Prove an email mismatch, revoked grant, or expired grant creates no session.
- Prove a valid callback stores the Auth0 subject and creates the existing
  digest-only session and cookie.
- Prove auditors are never provisioned into `users`.
- Preserve portal authorization, review-period filtering, grant revocation,
  logout, active legacy sessions, and audit-log secret exclusion.
- Use a fake auditor identity-provider adapter in integration tests; production
  tests must not depend on a live Auth0 tenant.

## Revisions

- 2026-07-27: Implemented the identity foundation with startup-resolved
  auditor endpoints, nonce-aware ID-token verification, secret-safe exchange
  inputs, and a six-digit, three-minute Auth0 passwordless policy documented in
  the environment runbook.
- 2026-07-24: Initial spec. Chose Auth0 Passwordless Email with hosted Universal
  Login, grant-bound Authorization Code Flow with PKCE, a separate auditor
  identity boundary, and retained Proofplane authorization and sessions.
