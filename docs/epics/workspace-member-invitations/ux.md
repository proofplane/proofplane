# Workspace Member Invitations UX

## Experience Principle

Member management is an authenticated product task. Keep it compact, familiar,
and calm: people should understand who can act in the workspace, which
invitations are pending, and what will happen before they confirm a destructive
or identity-sensitive action.

The interface uses the landing portal's existing authenticated shell, tokens,
buttons, fields, status panels, and inline confirmation patterns. It adds no
decorative motion and does not use modals.

## People Navigation And Layout

Add `People` to authenticated primary navigation and route it to `/app/people`.
The page heading is `People` with supporting copy `Manage workspace members and
pending invitations.`

The page contains, in order:

1. an inline invitation form;
2. pending invitations; and
3. current members.

Use compact ledger rows rather than a grid of cards. On narrow screens, each row
stacks its metadata and actions without horizontal scrolling. All controls keep
a minimum 44px target, visible focus, programmatic labels, and live status or
alert announcements.

## Invite An Administrator

The form label is `Email address`. Supporting text states `Invited people join
as administrators.` The primary action is `Send invitation`.

On success, show `Invitation ready for {email}` with delivery state and two
actions: `Copy link` and `Done`. Copying remains available even when delivery is
queued, delivered, or failed. Announce `Invitation link copied` without moving
focus.

Validation preserves the entered email. Inline errors distinguish invalid input,
an existing pending invitation, and an existing workspace member. Authorization
and unavailable workspace responses remain coarse.

## Pending Invitations

Each pending row shows the invited email, `Admin`, expiry, and one of `Email
queued`, `Email sent`, or `Email failed`. Actions are `Copy link`, `Send again`,
and `Revoke`.

`Copy link` requests current authority immediately before writing to the
clipboard. It does not change the expiry or invalidate an emailed link.

`Send again` uses inline confirmation: `Send a new invitation to {email}? The
previous link will stop working.` Confirmation changes to `Sending` while the
generation rotates. Success announces the new expiry and offers `Copy new link`.

`Revoke` uses inline confirmation: `Revoke this invitation? Its link will stop
working.` Remove the row only after the API confirms revocation. Failed actions
retain the row and explain that the existing invitation is unchanged.

When no invitations are pending, show `No pending invitations.` without a large
empty-state illustration.

## Current Members

Each member row shows name when available, email when available, role, and joined
date. Mark the signed-in user as `You`.

Managers may remove another member. The inline confirmation reads `Remove
{person} from this workspace? Their agent connections will no longer authorize
workspace access.` The primary destructive wording is `Confirm removal`; the
secondary action is `Cancel`.

Do not show a remove action for the signed-in user. If a last-owner or stale
membership conflict is returned, keep the row and show the server's stable
explanation.

## Invitation Acceptance

Invitation links target `/join#token={token}`. On first render, the portal moves
the token into `sessionStorage`, removes the fragment with
`history.replaceState`, and submits it to the preview endpoint. No subsequent
URL contains the token.

The preview page shows:

- eyebrow: `Workspace invitation`;
- heading: `Join {workspace_name}`;
- `You were invited as an administrator.`;
- the invited email and expiry; and
- primary action `Continue with Auth0`.

Starting authentication uses the invited email as `login_hint`, requests the
Proofplane API audience, and forces a fresh login so an unrelated existing Auth0
session cannot accept the invitation silently. Preserve `/join` as the callback
return destination while the token remains only in tab-scoped storage.

After authentication, preview the invitation again. With a matching verified
email, show `Join workspace` and `Cancel`. Acceptance happens only when the user
selects `Join workspace`. On success, clear stored authority, refresh workspace
state, and route to `/app/people`.

## Recovery States

- **Wrong account:** `This invitation was sent to {invited_email}. Sign out and
  continue with that account.` Provide `Use another account`.
- **Unverified email:** `Verify this email with your identity provider before
  joining the workspace.` Provide `Try again`.
- **Expired or revoked link:** `This invitation is no longer available. Ask a
  workspace administrator for a new one.` Do not reveal workspace details when
  preview itself is unavailable.
- **Already belongs elsewhere:** `This account already belongs to another
  workspace.` Do not change either membership.
- **Temporary API or Auth0 failure:** retain tab-scoped authority and provide a
  retry action.
- **Missing browser authority:** show the unavailable state and a link to the
  Proofplane home page.

Browser back, refresh, duplicate callback, repeated join, and two-tab acceptance
must lead to a stable success or one of these recovery states without exposing
the token.

## Accessibility And Motion

Use semantic forms, headings, lists or tables, and buttons. Move focus to the
first relevant error after submission only when the error is outside the current
control; keep focus on the initiating action for row-level outcomes. Status
changes use `role="status"`, failures use `role="alert"`, and confirmation copy
is associated with its controls.

Transitions may use the authenticated product's 150–250ms state feedback, but
must not delay interaction. Reduced motion removes transforms and preserves all
state and hierarchy.
