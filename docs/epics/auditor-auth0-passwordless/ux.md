# Auditor Auth0 Passwordless UX

## Experience Principle

The auditor should experience one continuous Proofplane journey even though
Auth0 hosts the identity challenge. Proofplane explains why authentication is
required and what will open; Auth0 handles only mailbox verification; the
auditor returns directly to the requested read-only portal.

## Primary Journey

### 1. Invitation

The existing invitation URL opens a Proofplane page:

- eyebrow: `Auditor verification`;
- heading: `Verify access for {auditor_email}`;
- explanation: `Proofplane will verify this email before opening the read-only
  evidence portal.`; and
- primary action: `Continue`.

Do not claim that Proofplane itself will send the code. Submitting the action
validates the invitation and redirects to Auth0.

### 2. Hosted verification

Auth0 Universal Login uses the Proofplane custom domain and branding. Pass the
grant email as `login_hint`. The hosted flow may show the prefilled identifier
before sending the code; it must never let a different authenticated email gain
access.

Customize the passwordless code screen where the Auth0 plan permits:

- title: `Verify your email`;
- description: `We sent a code to ${email}.`;
- code label or placeholder: `Verification code`;
- primary action: `Continue`;
- resend action: `Resend code`; and
- invalid-code message: `That code is invalid or expired. Try again or request
  a new code.`

Use `autocomplete="one-time-code"` and retain Auth0's accessible focus, status,
and error behavior. The email template uses Proofplane sender identity, subject
`Your Proofplane auditor access code`, the code, its actual configured expiry,
and an ignore-this-message notice.

### 3. Return

After successful authentication, Auth0 returns to the Proofplane callback.
Proofplane creates the local auditor session and immediately redirects to
`/auditor-access/portal`. Do not add a success interstitial.

## Returning Auditors

An active Proofplane auditor session continues directly to the portal and does
not visit Auth0. When no valid Proofplane session exists, starting from the
invitation requires fresh Auth0 authentication even if the browser has another
Auth0 tenant session.

Explicit logout ends only the Proofplane auditor session and returns the user to
a neutral signed-out page or the existing logout destination. It must not log
the browser out of unrelated Proofplane management-plane sessions.

## Failure States

- **Invalid invitation:** Preserve the existing unavailable page without
  confirming the workspace, grant, or email.
- **Authentication rejected:** Show `We couldn't verify this access request.
  Return to your invitation and try again.`
- **Auth0 unavailable:** Show `Email verification is temporarily unavailable.
  Please try again.` with a retry path back through the invitation.
- **Grant expired or revoked during login:** Use the same unavailable response
  as an invalid invitation.
- **Email mismatch:** Use the generic authentication-rejected response; never
  display both the expected and submitted addresses.

Do not automatically retry a callback or token exchange because the
authentication transaction is one-use.

## Responsive And Visual Requirements

- Use a Proofplane custom Auth0 domain where supported so the address bar
  remains visibly related to Proofplane.
- Match the portal's logo, typography, color, focus ring, and button hierarchy
  using Universal Login branding and templates.
- Keep the hosted form to one narrow column and one primary action.
- Verify the invitation, Auth0 code, callback failure, and portal transition at
  mobile and desktop widths.
- Avoid motion beyond brief status feedback and respect reduced-motion
  preferences.

## UX Acceptance

- An auditor can complete the flow without understanding Auth0 or OAuth.
- The expected email is clear before redirect and on the hosted code screen.
- Browser back, refresh, expired state, duplicate callback, and resend have a
  recoverable path.
- No page exposes invitation tokens, authorization codes, state, or internal
  failure details.
- Screen-reader and keyboard users receive labels, focus placement, error
  announcements, and resend status from the hosted experience.
