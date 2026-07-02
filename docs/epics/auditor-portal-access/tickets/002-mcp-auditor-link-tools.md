# 002 - MCP Auditor Link Tools

**Status:** Done · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#mcp-tools)

**Summary** - Expose auditor link creation and management through MCP so the
audited user can ask their agent to create, list, and revoke auditor access.

**Acceptance criteria**

- [x] Given authorized MCP credentials, when `create_auditor_access_link` is
  called, then it returns the one-time invite URL and non-secret grant metadata.
- [x] Given unauthorized credentials or a cross-workspace token, when any
  auditor link tool is called, then MCP returns a concealed authorization
  problem without leaking grant state.
- [x] Given list or revoke tool responses, when inspected, then raw invite
  secrets are absent.
- [x] Given existing MCP compliance tools, when this ships, then their schemas
  and authorization behavior are unchanged.

**Tasks**

- [x] Add create/list/revoke auditor link MCP tools.
- [x] Add input validation for auditor email and optional expiry.
- [x] Add MCP audit events for create and revoke.
- [x] Add tool schema assertions.
- [x] Add MCP integration tests for success, authorization, and secret
  exclusion.
