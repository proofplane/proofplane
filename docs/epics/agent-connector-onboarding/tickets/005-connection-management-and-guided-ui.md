# 005 - Connection Management And Guided UI

**Status:** Done · **Depends on:** 003, 004 · **Spec:** [spec.md](../spec.md#website-experience)

**Summary** - Replace the token-centric MCP preview with guided, menu-based
desktop setup, a simple account-level Proofplane grant, and a live list where
users can recognize and revoke their agent connections.

**Acceptance criteria**

- [x] Given a valid Proofplane OAuth transaction, when consent opens, then it
  asks whether to grant the escaped registered client access to Proofplane and
  displays no workspace, role, resource, or scope details.
- [x] Given a missing, expired, replayed, or invalid OAuth transaction, when
  consent is submitted, then it shows only generic recovery guidance.
- [x] Given a valid consent transaction, when the user cancels, then the
  request is consumed and the registered client receives `access_denied` with
  the original state.
- [x] Given a supported client, when a user selects it, then the UI launches or
  explains the verified native settings path without requiring a terminal or
  config edit.
- [x] Given completed authorizations, when the user returns to Proofplane,
  then the newest-first list shows only client, grant/use state,
  authorization time, and last use without claiming external client health.
- [x] Given an owned connection, when it is revoked, then it is marked revoked
  locally, immediately denied by MCP, removed from the list, audited to the
  user, and unrelated connections are unchanged.
- [x] Given an expired 24-hour access token, when the client cannot restart
  OAuth automatically, then the user receives an accurate reconnect path
  instead of a generic tool failure.
- [x] Given an unknown, revoked, or another user's connection, when revocation
  is attempted, then the API returns the same `404` without revealing its
  existence.

**Tasks**

- [x] Simplify server-rendered consent and implement state-preserving
  cancellation plus generic recovery.
- [x] Add authenticated user-scoped connection list and revoke operations,
  wiring, and audit emission.
- [x] Replace the incomplete stdio preview and stale token/permission modules
  with typed connection APIs and React Query states.
- [x] Add verified Claude/Cowork and Codex/ChatGPT desktop setup cards, MCP URL
  and prompt copy, and 24-hour reconnect guidance.
- [x] Add connection rows with inline confirmation, pending, error, empty,
  loading, and success states.
- [x] Add Rust integration/unit tests and frontend component/browser coverage.
- [x] Reconcile the spec, epic index, and [ux.md](../ux.md), then run all
  delivery validation.

**Notes**

- 2026-07-13: Removed the unused `ui/src/api/tokens.ts` frontend module and its
  isolated test. They referenced API-token endpoints removed in PR #42 and had
  no production consumers; the active CSS design-token module is unchanged.
- 2026-07-10: Implemented and validated. The product UI now follows a simple
  account-level grant presentation: workspace and scopes remain enforced by
  OAuth and MCP but are intentionally absent from consent and connection
  management. See the 2026-07-10 spec revision and [ux.md](../ux.md).
- 2026-07-09: `ppat_` API tokens were removed in PR #42, so this ticket no
  longer offers an "advanced API-token setup" path; OAuth is the only setup.
  Workspace membership remains the fixed internal grant boundary, but ticket
  005 intentionally removes it from the consent presentation.
- 2026-07-08: Proofplane owns MCP OAuth consent and tokens; Auth0 remains the
  upstream human login provider.
- 2026-07-02: OAuth connections use 24-hour access tokens and may require a
  visible reconnect when the client does not restart authorization.
- The harness remains authoritative for transport health and mounted tools.
