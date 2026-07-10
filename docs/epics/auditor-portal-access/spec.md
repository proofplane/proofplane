# Auditor Portal Access Spec

## Goal

Let a workspace user give a named auditor secure browser access to all controls,
evidence submissions, and eligible evidence attachments in one workspace,
without turning the auditor into a workspace member or issuing an API token.

## Trust Model

Auditor access is a workspace-scoped grant created for one email address. The
invite link is a bearer secret, but it is not enough to view data: the browser
user must prove control of the grant email with a one-time code before
Proofplane creates an auditor session.

The OTP proves current email control. The auditor session is the browser
credential used after verification. Sessions are server-side, revocable, and
valid for seven days with no refresh token. Every portal page and download
reloads the session and backing grant, then rejects expired, revoked, missing,
or cross-workspace state.

## Auditor Access Grants

Persist each grant with workspace, auditor email, creator (user or agent
connection — `ppat_` API tokens were removed in PR #42), created time, default
30-day expiry, optional revocation time, and a digest of the raw invite secret. The raw invite secret is returned only when the grant is
created and is never stored.

Grant creation, listing, and revocation require a new
`manage_auditor_access` workspace permission. Ordinary evidence/control read
tokens cannot create auditor shares.

## MCP Tools

The first management surface is MCP:

```text
create_auditor_access_link(email, expires_at?)
list_auditor_access_links()
revoke_auditor_access_link(grant_id)
```

The create response may contain the one-time invite URL. List and revoke
responses return only non-secret grant metadata. MCP responses and audit logs
must not include raw invite secrets except for the direct create response.

## OTP Verification

Opening an invite starts an email OTP flow. OTPs are single-use, expire after
10 minutes, and are rate-limited per grant/email. Store only OTP digests.

The mailer is a small adapter. Local and test environments capture outbound
messages for assertions; production delivery can be wired to a provider without
changing the portal contract.

Ticket 007 ships the browser invite flow as
`GET /auditor-access/{workspace_id}?token=...`, with server-rendered form posts
for OTP request and verification. The existing JSON OTP endpoints remain
available for API-style callers.

## Auditor Sessions

Successful OTP verification creates a server-side auditor session and sets an
opaque HttpOnly, Secure, SameSite cookie. Session rows store the grant,
auditor email, expiry, revocation, and last-used metadata. Logging must never
include session IDs or cookie values.

Session expiry is absolute at seven days. Reverification through a new OTP is
required after expiry. Revoking a grant immediately invalidates existing
sessions because every route checks the backing grant.

## Portal Read Model

The portal exposes all workspace controls, mapped Evidence Requests, all
historical Evidence Submissions, and attachment metadata in deterministic
order. This is intentionally broader than the old latest-only packet idea:
auditors need the submitted record, not a curated snapshot.

Ticket 005 ships the backend read model as
`GET /auditor-access/portal/data`, authenticated only by the auditor session
cookie. The endpoint uses the session workspace as its scope rather than a
workspace path parameter.

Non-archived attachment metadata includes download eligibility, filename,
content type, content length, checksums, and lifecycle status. Archived
attachments are filtered out and do not appear in the portal response. Internal
object keys and storage backend details are never serialized.

## Attachment Downloads

Verified auditor sessions may download uploaded, unarchived attachments.
Pending, finalizing, failed, malicious, archived, missing, or cross-workspace
attachments are not downloadable.

Ticket 006 ships the backend download route as
`GET /auditor-access/portal/evidence-submissions/{submission_id}/attachments/{attachment_id}/download`,
authenticated only by the auditor session cookie. It does not issue browser
API tokens or new attachment download grants for auditors.

Ticket 007 ships the server-rendered portal page as
`GET /auditor-access/portal`, authenticated by the same auditor session cookie
and backed by the existing portal read model.

Downloads stream through Proofplane, not directly from object storage. The
download path rechecks the session, underlying auditor access grant, workspace,
attachment status, and object metadata before streaming with safe
`Cache-Control`, `Referrer-Policy`, and `Content-Disposition` headers.

## Audit Logging

Grant creation, revocation, OTP send, OTP verification, session creation,
portal read, and attachment download emit structured audit logs with stable
identifiers. Logs must not include invite secrets, OTPs, session IDs, cookies,
object keys, attachment bytes, or free-text attachment contents.

## Deferred Work

Auditor comments, request/response workflows, review status tracking, bulk ZIP
exports, firm branding, and a separate SPA are deferred. The v1 portal is a
secure read-only browser surface.

Worker-backed auditor OTP mail delivery is deferred until production mail is
added. The worker should own OTP generation, digest persistence, and mail send
so raw OTP codes are never stored in durable queue payloads.

## Revisions

- 2026-07-06: Added deferred worker-backed OTP mail delivery so production
  mail wiring does not leave the request path responsible for provider retries.
- 2026-07-01: Replaced the stale packet/export plan with email-bound
  auditor portal access, OTP verification, seven-day sessions, and direct
  attachment downloads.
- 2026-07-02: Recorded the shipped portal read model endpoint and clarified
  that archived attachments are omitted from portal metadata.
- 2026-07-02: Recorded the shipped direct auditor attachment download route
  and clarified that it uses the auditor session cookie rather than a new
  attachment download grant.
- 2026-07-04: Recorded the shipped server-rendered browser invite and portal
  routes.
