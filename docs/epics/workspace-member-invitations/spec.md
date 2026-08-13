# Workspace Member Invitations Spec

## Purpose And Constraints

Allow an existing workspace owner or administrator to invite another person by
email and manage the resulting invitation from the authenticated portal. An
accepted invitation creates one administrator membership in the inviter's
workspace.

Proofplane remains authoritative for invitations, roles, and memberships.
Auth0 authenticates the person and proves control of the invited mailbox; Auth0
Organizations are not used. Users remain limited to one workspace, workspace
roles remain `owner` and `admin`, and ownership transfer is outside this epic.

Invite links remain a supported delivery method after email delivery ships.
Creating or sending an invitation must always leave the inviter with a link they
can copy and share manually.

## Identity And Authorization Boundary

Extend the management-plane Auth0 claims policy with a namespaced verified-email
claim. The claim contains a nonblank email plus verification state and is
required only when accepting a workspace invitation. Existing authenticated
routes continue to accept tokens that omit it.

Acceptance normalizes the authenticated email with the same contract used at
issuance and requires an exact match with the invitation. A display email,
`login_hint`, query parameter, or unverified profile claim never authorizes
membership.

Owners and administrators may create, resend, revoke, and copy workspace
invitations and may remove other members. Every invitation grants the `admin`
role. The workspace aggregate continues to authorize member removal and prevent
removal of the last owner.

## Workspace Invitation Lifecycle

`WorkspaceInvitation` is a separate aggregate because it exists before a user
or membership, has an independent expiry and delivery lifecycle, and is loaded
by a bearer authority rather than by workspace membership.

Persist at least:

- invitation ID and workspace ID;
- inviter user ID;
- normalized invited email and fixed `admin` role;
- positive generation, created time, expiry time, accepted time, revoked time,
  and accepting user ID;
- the generation most recently queued for delivery and the generation most
  recently delivered, with delivery timestamps and a coarse last failure state.

The lifecycle is pending, accepted, revoked, or expired. Expiry is derived from
the current time and `expires_at`; no background transition is required. An
invitation cannot be both accepted and revoked. Acceptance records one user and
is terminal.

Enforce at most one pending, unexpired invitation for a workspace and normalized
email. Creating a duplicate returns a stable conflict and the existing
invitation metadata; it does not silently rotate authority or send another
email.

Each generation expires seven days after issuance. Sending again increments the
generation, starts a new seven-day expiry, invalidates every older link, and
queues a fresh delivery. A stale expected generation cannot rotate a newer
invitation. Copying a link mints another token for the current generation and
does not rotate or invalidate existing links.

## Invitation Authority

Use a purpose-specific encrypted PASETO token and independent configured key
ring. The token carries only the invitation ID, generation, purpose, issued
time, and expiry. Verification requires the expected purpose, a supported key,
valid registered claims, and equality with the current persisted generation and
expiry.

The browser receives the invite token in the URL fragment. The landing portal
moves it immediately into tab-scoped session storage and removes it from the
address bar before navigation or Auth0 redirection. The token is submitted in
request bodies for preview and acceptance; it never appears in query strings,
logs, referrers, analytics, or error messages.

## Acceptance Transaction

Acceptance runs in one unit of work:

1. Verify and load the current invitation generation with a lock.
2. Reject an unavailable, expired, revoked, already-accepted-by-another-user, or
   email-mismatched invitation using coarse public errors.
3. Resolve the authenticated user's existing workspace membership under the
   same per-user serialization used by workspace creation.
4. Reject membership in another workspace. Treat an existing membership in the
   invited workspace as a successful replay only when it belongs to the same
   authenticated user.
5. Load the workspace aggregate, add the user as `admin`, save its complete
   snapshot, mark the invitation accepted, and commit atomically.

Concurrent acceptance has one winner. A successful same-user replay returns the
current workspace response without creating a duplicate membership.

## Reads And HTTP Contract

Add a purpose-built workspace people read model containing:

- the current workspace and actor role;
- members with user ID, display name, email, role, and joined time; and
- pending invitations with ID, invited email, role, generation, expiry,
  delivery state, and delivery timestamps.

Expose these authenticated management routes:

- `GET /workspace/people`;
- `POST /workspace/invitations` with `{ "email": string }`;
- `POST /workspace/invitations/{id}/link` to mint a current-generation link;
- `POST /workspace/invitations/{id}/resend` with
  `{ "expected_generation": number }`;
- `DELETE /workspace/invitations/{id}`; and
- the existing `DELETE /workspace/members/{user_id}`.

Creation and resend return invitation metadata plus the copyable URL. The URL is
the only response field containing the bearer token and must use the configured
landing-portal base URL.

Expose these public invitation routes:

- `POST /workspace-invitations/preview` with `{ "token": string }`; and
- authenticated `POST /workspace-invitations/accept` with
  `{ "token": string }`.

Preview returns only the workspace name, invited email, fixed role, and expiry
after full token and invitation validation. Invalid, expired, revoked, foreign,
or stale authority produces the same unavailable response. Acceptance returns
the existing workspace-with-role response.

Stable management conflicts distinguish a duplicate pending invitation, stale
resend generation, existing membership in another workspace, and last-owner
removal. Authorization and cross-workspace references remain concealed.

## Email Delivery

Creating and resending an invitation commits a versioned
`SendWorkspaceInvitation` command in the outbox with only invitation ID and
generation. The message contains no email address, invite token, or URL.

The worker reloads the invitation, ignores a stale generation or non-pending
invitation, mints the current link, and sends a transactional message through a
mail adapter. Resend is the production implementation. Use
`workspace-invitation/{invitation_id}/{generation}` as the provider idempotency
key so at-least-once delivery does not duplicate the same generation within the
provider's idempotency window.

Persist delivery success or a coarse failure classification only if the command
still matches the current generation. Provider failures follow the existing
worker retry policy. Exhausted delivery leaves the invitation and copyable link
usable and visible as delivery failed; a manager can send again to create a new
generation.

Configuration supplies the landing-portal base URL, invitation PASETO key ring,
Resend API key, and sender identity. Secrets must be redacted from debug output.
Local tests use a capturing mail adapter and do not call Resend.

## Audit And Sensitive Data

Emit success audit events after commit for invitation creation, resend,
revocation, acceptance, and member removal. Include workspace ID, actor user ID,
invitation ID or target user ID, request ID, operation, and outcome where
applicable.

Never emit invited or authenticated email addresses, invite tokens or URLs,
authorization headers, PASETO keys, Resend credentials, or provider response
bodies. Operational delivery logs and metrics use bounded outcomes and provider
status classes without tenant identifiers or addresses.

## Deployment And Compatibility

The application is not deployed, so add invitation tables and constraints to
the resettable baseline migration. Existing users, memberships, workspace
creation, login, agent connections, auditor invitations, and member removal
contracts remain unchanged.

Production setup requires a verified Resend sending subdomain with SPF and DKIM,
DMARC policy, a restricted API key, configured sender identity, and an Auth0
Action that adds the namespaced email and verification claims to Proofplane API
access tokens. The production runbook must identify the owners of Auth0 claim
configuration, Resend credentials, DNS, template content, and delivery review.

## Verification

Use domain tests for lifecycle invariants and token claims; Postgres-backed tests
for snapshot round trips, locks, concurrency, uniqueness, and rollback; and
integration-v2 tests for client-visible HTTP, worker, email-capture, audit, and
tenant-concealment behavior. Landing-portal component and browser tests cover
the inviter and invitee journeys described in `ux.md`.

Before completing implementation, run `make check` in Proofplane and
`npm run build`, `npm test`, and `npm run test:smoke` in `landing-portal`.

The production prerequisites and production-like smoke evidence are governed by
the [workspace invitation production runbook](../../workspace-invitation-production-runbook.md).
They remain a human release gate because they require authorized access to
Auth0, Resend, DNS, the deployment secret path, and the target environment.
