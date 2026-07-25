# 003 - Hosted Auditor Login Cutover

**Status:** Todo · **Depends on:** 001, 002 · **Spec:**
[spec.md](../spec.md#callback-and-oidc-verification)

**Summary** - Complete the Auth0 callback, bind the verified mailbox to the
active grant, issue the existing local auditor session, and switch invitations
to the branded Universal Login journey.

**Acceptance criteria**

- [ ] Given a valid one-use transaction and matching verified Auth0 identity,
      when the callback completes, then a session containing the Auth0 subject
      is created and the auditor is redirected to the read-only portal.
- [ ] Given missing callback input, rejected or unavailable Auth0 exchange,
      invalid nonce, unverified or mismatched email, or inactive grant, when the
      callback runs, then no session is created and a coarse retry or
      unavailable response is rendered.
- [ ] Given a valid callback is replayed or submitted concurrently, when both
      requests run, then at most one can use the transaction and create a
      session.
- [ ] Given a legacy session created before cutover, when it remains active,
      then portal access, grant revocation, logout, and review-period filtering
      continue unchanged.
- [ ] Given a successful or failed login, when logs and audits are inspected,
      then stable lifecycle events exist and no invitation, OIDC, PKCE, email,
      or session secret is present.

**Tasks**

- [ ] Add nullable `auditor_sessions.auth0_subject` through an additive
      migration and require it for new Auth0-created sessions.
- [ ] Implement code exchange with client authentication, exact redirect URI,
      and the recorded PKCE verifier.
- [ ] Validate nonce and exact normalized grant-email binding before session
      creation.
- [ ] Add login-start and callback routes with sanitized error mapping and
      lifecycle audit events.
- [ ] Update the invitation page and branded Auth0/email copy per
      [ux.md](../ux.md).
- [ ] Preserve local-only logout and force fresh Auth0 login on the next start.
- [ ] Add fake-provider integration coverage for success, rejection,
      unavailability, replay, legacy sessions, and secret-free observability.

**Notes**

- The Proofplane session remains the portal authorization mechanism; see
  [Auditor Sessions](../spec.md#auditor-sessions).
