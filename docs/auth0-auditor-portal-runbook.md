# Auth0 Auditor Portal Runbook

This runbook configures the Auth0 application that authenticates external
auditors by email OTP. Auth0 proves mailbox control; Proofplane continues to
authorize invitation grants and never provisions an auditor into the
management-plane `users` table.

Repeat the setup for every environment. Keep development and production client
credentials separate.

## Proofplane Configuration

Configure the dedicated auditor client under `auth0.auditor_portal`:

```yaml
auth0:
  auditor_portal:
    client_id: "<regular-web-application client ID>"
    client_secret: "<secret-managed client secret>"
    callback_path: "/auditor-access/auth0/callback"
    connection: "email"
```

Store `client_secret` in the environment's secret manager and render it into
the runtime configuration at deployment. Do not place a production secret in
source control, deployment logs, or support material.

The API derives these values at startup:

- Callback URL: the environment's `server.public_api_base_url` plus
  `/auditor-access/auth0/callback`.
- Authorization endpoint: the configured Auth0 issuer plus `/authorize`.
- Token endpoint: the configured Auth0 issuer plus `/oauth/token`.
- ID-token audience: the auditor application's client ID.

Startup must fail if any auditor client field is missing or blank, the
connection is not `email`, or the callback leaves the `/auditor-access/`
namespace.

## Auth0 Application

1. Create an Auth0 **Regular Web Application** dedicated to the auditor portal.
   Do not reuse the management-plane or MCP client.
2. Select **RS256** as the signing algorithm.
3. Enable Authorization Code Flow. Proofplane adds PKCE S256 during login.
4. Add the exact derived callback URL for the environment to **Allowed Callback
   URLs**. Do not add wildcard callbacks.
5. Enable only the Passwordless Email connection for this application. Do not
   enable database, social, enterprise, or organization-based login.
6. Leave Auth0 Organizations disabled for this flow.
7. Record the client ID in configuration and deliver the client secret through
   the environment's secret manager.

The passwordless connection may create an Auth0 `email` identity when an
auditor first authenticates. That identity is not a Proofplane user and does
not grant access without an active Proofplane invitation transaction and grant.

## Passwordless Email

Under **Authentication > Passwordless > Email**:

1. Enable email OTP and enable the auditor application on the connection's
   Applications tab.
2. Allow signups so invited mailboxes can authenticate without pre-provisioning
   Auth0 users.
3. Select one-time code rather than magic-link delivery.
4. Set the OTP length to **6 digits** and expiry to **180 seconds**.
5. Configure a sender on a domain controlled by Proofplane and customize the
   subject and message so they identify Proofplane and state that the code is
   short-lived and single-use.

Auth0 accepts only the most recently issued code, invalidates it after use, and
allows three failed entries before a new code is required. Revisit the length
and expiry together if the three-minute lifetime changes. See
[Passwordless Authentication with Email](https://auth0.com/docs/authenticate/passwordless/authentication-methods/email-otp).

## Email, Domain, And Branding

1. Configure a production SMTP provider under **Branding > Email Provider**.
   Auth0's built-in provider is for testing only. Verify the sender domain's
   SPF and DKIM records and complete a real delivery test. See
   [External SMTP Email Providers](https://auth0.com/docs/customize/email/smtp-email-providers).
2. Configure the Proofplane custom Auth0 domain where the tenant plan supports
   it, complete DNS verification, and use that domain consistently as the
   configured issuer/JWKS host.
3. Apply Proofplane logo, colors, support links, and accessible copy to
   Universal Login and the passwordless email template. Preserve Auth0's focus,
   status, and one-time-code autocomplete behavior.
4. Confirm that the hosted login shows only the email passwordless journey for
   the auditor client.

## Attack Protection

Under **Security > Attack Protection**:

1. Enable **Brute-force Protection**, retain an effective blocking response,
   and configure administrator/user notifications as appropriate.
2. Enable **Suspicious IP Throttling** and enable its high-velocity traffic
   blocking response.
3. Verify both features report **Enabled**, not monitoring-only. Enabling a
   detector without a response records events but does not block traffic.
4. Review allowlisted IP ranges and keep the list empty unless an approved
   operational requirement exists.
5. Route Auth0 attack-protection and authentication logs into the environment's
   normal monitoring process.

See Auth0's [Brute-force Protection](https://auth0.com/docs/secure/attack-protection/brute-force-protection)
and [Suspicious IP Throttling](https://auth0.com/docs/secure/attack-protection/suspicious-ip-throttling)
documentation for the current dashboard controls.

## Environment Verification

Before enabling auditor traffic:

- Start the API with the environment configuration and confirm it resolves the
  expected callback, authorization endpoint, token endpoint, and client
  audience without logging the client secret.
- Confirm the exact callback URL is allowlisted and a near-match is rejected.
- Start from a valid Proofplane invitation and confirm Auth0 shows the branded
  email-only Universal Login experience.
- Confirm an OTP arrives through the production SMTP provider, expires after
  three minutes, and cannot be reused.
- Confirm the resulting ID token is RS256 and contains the expected issuer,
  auditor client audience, nonce, nonblank `sub` and `email`, and
  `email_verified: true`.
- Confirm management-plane and MCP login continue to use their existing clients
  and audiences.
- Confirm failed authentication logs contain no invitation token, authorization
  code, ID token, nonce, PKCE verifier, email address, or client secret.

Record the Auth0 tenant, custom domain, application ID, callback URL, connection
name, SMTP owner, OTP policy, and attack-protection reviewer in the
environment's deployment inventory. Never record the client secret there.
