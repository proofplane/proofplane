# 005 - MCP Setup Preview

**Status:** Done - Will Do Later · **Depends on:** 004 · **Spec:** [spec.md](../spec.md#mcp-setup-preview)

**Summary** - After token creation, show honest MCP installation and
configuration guidance with copyable snippets, readiness labels, and suggested
agent prompts.

**Acceptance criteria**

- [ ] Given a newly issued token, when the user reaches MCP setup, then they see
  copyable environment and client-config snippets that use the token safely.
- [ ] Given MCP backend work is not complete, when setup is displayed, then the
  UI labels the section as preview or waiting on MCP Server rather than ready.
- [ ] Given a copy action fails, when the browser denies clipboard access, then
  the UI shows a fallback without hiding the snippet.
- [ ] Given a user skips MCP setup, when they continue, then they can still reach
  the workspace home.

**Tasks**

- [ ] Build MCP setup route/section after token success.
- [ ] Add readiness labels for ready, preview, and waiting states.
- [ ] Add copyable env var and config snippets.
- [ ] Add suggested prompt list from `ux.md`.
- [ ] Add component tests for readiness labels and clipboard fallback.

**Notes**

- This ticket does not implement the MCP runtime. It links to the MCP Server
  epic and must avoid claiming production readiness early.
- Postponed until the MCP is more feature complete; revalidate the linked spec
  and UX before reopening because the current requirements may no longer apply.
