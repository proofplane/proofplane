# 002 - MCP Auditor Link Tools

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#mcp-tools)

**Summary** - Expose auditor link creation and management through MCP so the
audited user can ask their agent to create, list, and revoke auditor access.

**Acceptance criteria**

- [ ] Given authorized MCP credentials, when `create_auditor_access_link` is
  called, then it returns the one-time invite URL and non-secret grant metadata.
- [ ] Given unauthorized credentials or a cross-workspace token, when any
  auditor link tool is called, then MCP returns a concealed authorization
  problem without leaking grant state.
- [ ] Given list or revoke tool responses, when inspected, then raw invite
  secrets are absent.
- [ ] Given existing MCP compliance tools, when this ships, then their schemas
  and authorization behavior are unchanged.

**Tasks**

- [ ] Add create/list/revoke auditor link MCP tools.
- [ ] Add input validation for auditor email and optional expiry.
- [ ] Add MCP audit events for create and revoke.
- [ ] Add tool schema assertions.
- [ ] Add MCP integration tests for success, authorization, and secret
  exclusion.
