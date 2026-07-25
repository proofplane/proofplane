# Auditor Auth0 Passwordless Epic

Move auditor mailbox verification to Auth0 Passwordless Email while keeping
Proofplane authoritative for invitation secrets, grant scope, review periods,
revocation, and portal sessions. The core principle is that Auth0 authenticates
the auditor identity, while Proofplane alone authorizes access to evidence.

Full rationale, protocol contracts, schema, migration, and security decisions
live in [spec.md](./spec.md). Hosted-login appearance and behavior live in
[ux.md](./ux.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Auth0 Auditor Identity Foundation](./tickets/001-auth0-auditor-identity-foundation.md) | Todo | Add dedicated client configuration, claims verification, adapter boundaries, and the environment runbook. |
| 002. [Grant-Bound Authentication Transactions](./tickets/002-grant-bound-auth-transactions.md) | Todo | Persist one-use state, nonce, and PKCE material and construct secure authorization starts. |
| 003. [Hosted Auditor Login Cutover](./tickets/003-hosted-auditor-login-cutover.md) | Todo | Complete callbacks, issue local sessions, and switch the browser journey to branded Universal Login. |
| 004. [Custom OTP Retirement](./tickets/004-custom-otp-retirement.md) | Todo | Remove obsolete OTP routes, persistence, mail delivery, configuration, and tests after the cutover settles. |

## Sequencing

- **001** establishes the separate auditor identity boundary and can begin in
  parallel with the additive schema portion of **002**.
- **002** depends on 001's configuration contract and provides the secure
  grant-bound state required by the browser flow.
- **003** depends on 001 and 002, then performs the user-visible cutover while
  preserving active legacy sessions.
- **004** depends on 003 and must ship only after every API instance uses Auth0
  and the final custom OTP has exceeded its ten-minute lifetime.
