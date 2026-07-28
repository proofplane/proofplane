# 003 - Hosted Auditor Login Cutover

**Status:** Done · **Depends on:** 001, 002 · **Spec:**
[spec.md](../spec.md#callback-and-oidc-verification)

**Summary** - Complete the Auth0 callback, bind the verified mailbox to the
active grant, issue the existing local auditor session, and switch invitations
to the branded Universal Login journey.

**Acceptance criteria**

- [x] Given a valid one-use transaction and matching verified Auth0 identity,
      when the callback completes, then a session containing the Auth0 subject
      is created and the auditor is redirected to the read-only portal.
- [x] Given missing callback input, rejected or unavailable Auth0 exchange,
      invalid nonce, unverified or mismatched email, or inactive grant, when the
      callback runs, then no session is created and a coarse retry or
      unavailable response is rendered.
- [x] Given a valid callback is replayed or submitted concurrently, when both
      requests run, then at most one can use the transaction and create a
      session.
- [x] Given a successful or failed login, when logs and audits are inspected,
      then stable lifecycle events exist and no invitation, OIDC, PKCE, email,
      or session secret is present.

**Tasks**

- [x] Require a nonblank `auditor_sessions.auth0_subject` in the resettable
      baseline schema.
- [x] Implement code exchange with client authentication, exact redirect URI,
      and the recorded PKCE verifier.
- [x] Validate nonce and exact normalized grant-email binding before session
      creation.
- [x] Add login-start and callback routes with sanitized error mapping and
      lifecycle audit events.
- [x] Update the invitation page and branded Auth0/email copy per
      [ux.md](../ux.md).
- [x] Preserve local-only logout and force fresh Auth0 login on the next start.
- [x] Add fake-provider integration coverage for success, rejection,
      unavailability, replay, and secret-free observability.

**Notes**

- The Proofplane session remains the portal authorization mechanism; see
  [Auditor Sessions](../spec.md#auditor-sessions).
- The 2026-07-27 spec revision records the shipped callback, required-subject
  session, failure, and observability behavior.
