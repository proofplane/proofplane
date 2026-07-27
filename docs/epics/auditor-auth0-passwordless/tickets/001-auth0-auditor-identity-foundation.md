# 001 - Auth0 Auditor Identity Foundation

**Status:** Done · **Depends on:** none · **Spec:**
[spec.md](../spec.md#identity-and-authorization-boundaries)

**Summary** - Add a dedicated Auth0 auditor client and identity-provider
boundary that verifies passwordless identities without provisioning external
auditors as management-plane users.

**Acceptance criteria**

- [x] Given valid auditor Auth0 configuration, when the API starts, then the
      dedicated client, callback, connection, token endpoint, and verifier are
      available without exposing the client secret.
- [x] Given missing, blank, malformed, or escaping callback configuration, when
      startup validates it, then startup fails with field-specific errors and
      no secret value is rendered.
- [x] Given a valid Auth0 auditor ID token, when the auditor verifier checks it,
      then it returns a non-empty subject and verified email under the dedicated
      client audience.
- [x] Given an invalid algorithm, signature, issuer, audience, lifetime,
      subject, email, or `email_verified` claim, when verification runs, then
      the identity is rejected without JIT-provisioning a `users` row.
- [x] Given existing management-plane and MCP Auth0 tokens, when this ticket
      ships, then their audiences, claims policies, and provisioning behavior
      are unchanged.

**Tasks**

- [x] Add and validate `auth0.auditor_portal` configuration with secret-safe
      debug behavior.
- [x] Define the auditor identity and Auth0 exchange/verifier adapter boundary.
- [x] Implement the dedicated ID-token claims policy on existing JWKS support.
- [x] Classify identity rejection separately from Auth0 or JWKS unavailability.
- [x] Add unit tests for configuration, redaction, accepted claims, and every
      rejection class.
- [x] Document Auth0 application, passwordless connection, SMTP, custom domain,
      branding, callback, and attack-protection setup.

**Notes**

- Do not route auditor identities through `UserAuthenticator`; see
  [Identity And Authorization Boundaries](../spec.md#identity-and-authorization-boundaries).
- The 2026-07-27 spec revision records the shipped foundation and Auth0 OTP
  policy.
