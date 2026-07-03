# 002 - Workspace Grant And Runtime Authorization

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#workspace-grant-bridge)

**Summary** - Bind each Auth0 MCP access token to the one active Proofplane
workspace connection for its user/client/resource tuple. Reuse that connection
on repeated authorization and enforce it on every MCP request.

**Acceptance criteria**

- [ ] Given an interactive Auth0 MCP authorization, when the Redirect Action
  runs, then the user can approve one currently accessible workspace and the
  resulting access token names that connection, workspace, client, resource,
  and approved scopes.
- [ ] Given tampered, expired, replayed, wrong-state, wrong-client, or
  inaccessible-workspace consent, when the bridge handles it, then no
  active connection or usable credential is created.
- [ ] Given workspace approval followed by denied Auth0 consent or abandoned
  code exchange, when the pending deadline passes, then no authorized
  connection remains.
- [ ] Given an active connection and repeated authorization, when the Action
  runs, then it rechecks membership and scopes and adds the existing connection
  claims without another Proofplane workspace redirect.
- [ ] Given no reusable connection during `prompt=none`, when the Action runs,
  then authorization fails with `interaction_required` instead of bypassing
  workspace consent.
- [ ] Given a valid Auth0 access token, when an MCP tool runs, then Proofplane
  requires the signed claims to match an active connection and current
  workspace membership.
- [ ] Given membership removal or local connection revocation, when an
  existing access token is used or authorization is repeated, then access
  fails immediately and the revoked connection is not reused.
- [ ] Given successful authorization, reuse, rejection, use, or revocation,
  when audited, then events identify the agent connection without credentials.

**Tasks**

- [ ] Implement the post-login Action for interactive redirects, continuation,
  active-connection lookup, and access-token claims.
- [ ] Add the signed, single-use consent contract and secure workspace
  picker with approve/deny handling.
- [ ] Add transactional pending/active agent-connection and audit persistence
  with a unique active user/client/resource constraint.
- [ ] Add internal Action endpoints for grant creation and reusable-connection
  validation.
- [ ] Enforce Auth0 claim-to-connection and live membership checks in MCP.
- [ ] Activate pending connections on first valid MCP use and expire abandoned
  grants.
- [ ] Add authoritative local revocation and optional Auth0 user-grant cleanup.
- [ ] Add Action contract, route, repository, runtime, isolation, replay,
  connection-reuse, expiry, and revocation tests.

**Notes**

- 2026-07-02: The spec removes `offline_access`; repeated authorization looks
  up the one active user/client/resource connection and reuses it when safe.
- Auth0 Organizations are excluded because current third-party application
  support does not satisfy the MCP client model.
