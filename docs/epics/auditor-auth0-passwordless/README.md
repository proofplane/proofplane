# Auditor Auth0 Passwordless Epic

Move auditor mailbox verification to Auth0 Passwordless Email while keeping
Proofplane authoritative for invitation secrets, grant scope, review periods,
revocation, and portal sessions. The core principle is that Auth0 authenticates
the auditor identity, while Proofplane alone authorizes access to evidence.

Full rationale, protocol contracts, schema, migration, and security decisions
live in [spec.md](./spec.md). Hosted-login appearance and behavior live in
[ux.md](./ux.md). Environment setup is recorded in the
[Auth0 auditor portal runbook](../../auth0-auditor-portal-runbook.md).

## Tracking

Tickets live on GitHub, not in this directory. This epic is complete.

- Epic: [#89 Epic: Auditor Auth0 Passwordless](https://github.com/proofplane/proofplane/issues/89)
- Tickets: attached to that issue as sub-issues, and labeled
  [`epic:auditor-auth0-passwordless`](https://github.com/proofplane/proofplane/issues?q=is%3Aissue+label%3Aepic%3Aauditor-auth0-passwordless)

The epic issue carries the ticket index, status, and sequencing. See
[`docs/agents/issue-tracker.md`](../../agents/issue-tracker.md) for the
workflow.
