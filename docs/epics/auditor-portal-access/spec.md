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

Attachment metadata includes download eligibility, filename, content type,
content length, checksums, and lifecycle status. Internal object keys and
storage backend details are never serialized.

## Attachment Downloads

Verified auditor sessions may download uploaded, unarchived attachments.
Pending, finalizing, failed, malicious, archived, missing, or cross-workspace
attachments are not downloadable.

Downloads stream through Proofplane, not directly from object storage. The
download path rechecks session, grant, workspace, attachment status, and object
metadata before streaming with safe `Cache-Control`, `Referrer-Policy`, and
`Content-Disposition` headers.

## Audit Logging

Grant creation, revocation, OTP send, OTP verification, session creation,
portal read, and attachment download emit structured audit logs with stable
identifiers. Logs must not include invite secrets, OTPs, session IDs, cookies,
object keys, attachment bytes, or free-text attachment contents.

## Deferred Work

Auditor comments, request/response workflows, review status tracking, bulk ZIP
exports, firm branding, and a separate SPA are deferred. The v1 portal is a
secure read-only browser surface.

## Revisions

- 2026-07-01: Replaced the stale packet/export plan with email-bound
  auditor portal access, OTP verification, seven-day sessions, and direct
  attachment downloads.
