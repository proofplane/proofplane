# 006 - Generic Client Fallback

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#other-clients)

**Summary** - Provide accurate remote-MCP guidance for clients outside the
first-class Claude/Cowork and Codex paths, generated from one maintained
connection definition.

**Acceptance criteria**

- [ ] Given a generic MCP client without a reviewed Auth0 registration path,
  when the user follows fallback guidance, then it uses advanced API-token
  setup rather than an unsupported OAuth promise.
- [ ] Given an unsupported or incompatible client, when selected, then
  Proofplane identifies the limitation instead of claiming support.
- [ ] Given an advanced bearer-token client, when this ships, then documented
  manual configuration remains available outside the default non-technical
  flow.
- [ ] Given Claude/Cowork or Codex setup, when generic guidance changes, then
  their first-class distribution artifacts are unchanged.

**Tasks**

- [ ] Define a versioned source for generic connection metadata.
- [ ] Add generic Streamable HTTP API-token guidance.
- [ ] Document that open Auth0 Dynamic Client Registration remains deferred.
- [ ] Add a client capability/support matrix with last-verified metadata.
- [ ] Validate generated snippets in tests or release checks.
- [ ] Document update ownership when a client changes its configuration format.

**Notes**

- Cursor and VS Code distribution are excluded.
- 2026-06-29: Scope narrowed with the spec to remove client-specific
  distribution.
- 2026-07-02: Open Auth0 Dynamic Client Registration remains deferred until
  tenant ACL, abuse prevention, and default API permissions are specified.
