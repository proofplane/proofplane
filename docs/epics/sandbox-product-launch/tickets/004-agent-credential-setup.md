# 004 - Agent Credential Setup

**Status:** Todo · **Depends on:** 001, auth-hierarchy-api/003, mcp-server/001 · **Spec:** [spec.md](../spec.md#product-api-gaps)

**Summary** - Let an owner issue and configure the sandbox AI-agent credential
without weakening one-time secret handling.

**Acceptance criteria**

- [ ] Given an authorized owner/admin, when a credential is issued, then the raw
  key is shown exactly once with API and MCP setup guidance.
- [ ] Given a page refresh or later credential list, when viewed, then the raw
  key cannot be recovered from the server or browser.
- [ ] Given an unauthorized caller or actor from another workspace, when
  credential management is attempted, then `404` is returned.
- [ ] Given analytics and logs, when issuance occurs, then no raw key, hash, or
  authorization header is captured.

**Tasks**

- [ ] Build actor list/create and credential issue/revoke UI.
- [ ] Add one-time secret display with explicit dismissal.
- [ ] Add API/MCP configuration snippets without embedding persisted secrets.
- [ ] Add browser and integration tests for rotation and secret non-retention.

**Notes**

- One-time credential behavior is specified in
  [ux.md](../ux.md#api-and-agent-moment).
