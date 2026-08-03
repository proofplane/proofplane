# Integration-v2 Reconciliation Epic

Bring the black-box integration suite forward to current `main`: remove its
retired mailer assumptions, exercise auditor authentication through the Auth0
boundary, and restore public-flow coverage for agent-native evidence and policy
document uploads. The core principle is to recover observable guarantees
without reviving production compatibility code or the deleted legacy suite.

Full rationale, coverage boundaries, and completion requirements live in
[spec.md](./spec.md), the source of technical depth. Tickets below are lean
handoff units that link into it.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Auth0 Harness And Auditor Coverage](./tickets/001-auth0-harness-and-auditor-coverage.md) | Todo | Restore compilation and replace obsolete OTP arrangements with current hosted-login coverage. |
| 002. [Agent Evidence Upload Coverage](./tickets/002-agent-evidence-upload-coverage.md) | Todo | Cover MCP preparation, raw transfer, retries, isolation, and lifecycle completion. |
| 003. [Agent Policy Upload Coverage](./tickets/003-agent-policy-upload-coverage.md) | Todo | Cover the policy-specific transfer contract and single-current-document races. |

## Sequencing

- **001** is foundational because it restores the shared harness and makes the
  integration-v2 target compile against current application dependencies.
- **002** depends on 001 and establishes small reusable helpers for executing a
  machine transfer descriptor without hiding the declaration or authority.
- **003** depends on 001 and 002 so it can reuse those transport mechanics while
  keeping policy-specific preparation, conflicts, and assertions explicit.
