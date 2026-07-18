# 004 - MCP Policy Document Grants

**Status:** Todo · **Depends on:** [002](./002-mcp-policy-catalog-tools.md), [003](./003-policy-document-lifecycle.md) · **Spec:** [spec.md](../spec.md#delegated-browser-management)

**Summary** - Add `manage_policy_document` so an authorized MCP connection
can give a human a short-lived browser session without moving file bytes or
credentials through MCP.

**Acceptance criteria**

- [ ] Given `write_controls` and an active policy, when the tool is called,
  then it returns a five-minute, single-use bearer URL classified for human
  browser use.
- [ ] Given a valid unredeemed URL, when opened, then it is atomically consumed
  and establishes an HttpOnly, SameSite, policy-scoped session bounded by the
  grant expiry.
- [ ] Given insufficient permission or malformed, expired, redeemed, archived,
  missing, or cross-workspace state, when issuance or redemption is attempted,
  then no policy existence or secret is leaked.
- [ ] Given tool responses, audit logs, and application logs, when inspected,
  then they contain no file bytes, raw token, full bearer URL, cookie, or object
  key.
- [ ] Given evidence document grants and sessions, when policy grants ship,
  then their claims, purpose, cookie scope, and behavior remain unchanged.

**Tasks**

- [ ] Add policy upload-grant persistence and atomic redemption operations.
- [ ] Add policy-specific typed claims and purpose separation to the existing
  upload-grant keyring.
- [ ] Add `manage_policy_document` request/response schemas and MCP handler.
- [ ] Add grant redemption and policy-scoped browser session routes/cookie.
- [ ] Emit identifier-only grant issuance and redemption audit events.
- [ ] Add MCP and HTTP integration tests for authorization, single use, expiry,
  cookie attributes, generic failures, secrets, and evidence regressions.
