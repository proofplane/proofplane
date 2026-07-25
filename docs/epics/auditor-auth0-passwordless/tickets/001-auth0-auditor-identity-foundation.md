# 001 - Auth0 Auditor Identity Foundation

**Status:** Todo · **Depends on:** none · **Spec:**
[spec.md](../spec.md#identity-and-authorization-boundaries)

**Summary** - Add a dedicated Auth0 auditor client and identity-provider
boundary that verifies passwordless identities without provisioning external
auditors as management-plane users.

**Acceptance criteria**

- [ ] Given valid auditor Auth0 configuration, when the API starts, then the
      dedicated client, callback, connection, token endpoint, and verifier are
      available without exposing the client secret.
- [ ] Given missing, blank, malformed, or escaping callback configuration, when
      startup validates it, then startup fails with field-specific errors and
      no secret value is rendered.
- [ ] Given a valid Auth0 auditor ID token, when the auditor verifier checks it,
      then it returns a non-empty subject and verified email under the dedicated
      client audience.
- [ ] Given an invalid algorithm, signature, issuer, audience, lifetime,
      subject, email, or `email_verified` claim, when verification runs, then
      the identity is rejected without JIT-provisioning a `users` row.
- [ ] Given existing management-plane and MCP Auth0 tokens, when this ticket
      ships, then their audiences, claims policies, and provisioning behavior
      are unchanged.

**Tasks**

- [ ] Add and validate `auth0.auditor_portal` configuration with secret-safe
      debug behavior.
- [ ] Define the auditor identity and Auth0 exchange/verifier adapter boundary.
- [ ] Implement the dedicated ID-token claims policy on existing JWKS support.
- [ ] Classify identity rejection separately from Auth0 or JWKS unavailability.
- [ ] Add unit tests for configuration, redaction, accepted claims, and every
      rejection class.
- [ ] Document Auth0 application, passwordless connection, SMTP, custom domain,
      branding, callback, and attack-protection setup.

**Notes**

- Do not route auditor identities through `UserAuthenticator`; see
  [Identity And Authorization Boundaries](../spec.md#identity-and-authorization-boundaries).
