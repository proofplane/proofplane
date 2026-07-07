# 008 - Generic Client Fallback

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#other-clients)

**Summary** - Provide accurate remote-MCP guidance for clients outside the
first-class Claude/Cowork and Codex paths, generated from one maintained
connection definition. Clients with compatible OAuth discovery and Auth0
registration behavior can use the hosted OAuth path; unsupported clients stay
on advanced API-token setup.

**Acceptance criteria**

- [ ] Given a generic MCP client without a supported Auth0 registration path,
  when the user follows fallback guidance, then it uses advanced API-token
  setup rather than an unsupported OAuth promise.
- [ ] Given a generic MCP client with a supported Auth0 DCR or manual
  registration path, when the user follows OAuth guidance, then the hosted MCP
  URL is used without a Proofplane-issued client ID or local client allowlist.
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
- [ ] Document that production Auth0 Dynamic Client Registration remains
  controlled until tenant ACL, abuse controls, cleanup ownership, monitoring,
  and default third-party API permissions are specified.
- [ ] Add a client capability/support matrix with last-verified metadata.
- [ ] Validate generated snippets in tests or release checks.
- [ ] Document update ownership when a client changes its configuration format.

**Notes**

- Cursor and VS Code distribution are excluded.
- 2026-06-29: Scope narrowed with the spec to remove client-specific
  distribution.
- 2026-07-02: Production Auth0 Dynamic Client Registration remains deferred
  until tenant ACL, abuse prevention, and default API permissions are
  specified.
- 2026-07-07: Development DCR is now validated with Codex, so generic guidance
  should distinguish clients that can use OAuth discovery/DCR from clients that
  still need advanced `ppat_` configuration.
