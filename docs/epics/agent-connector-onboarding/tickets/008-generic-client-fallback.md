# 008 - Generic Client Fallback

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#other-clients)

**Summary** - Provide accurate remote-MCP guidance for OAuth/DCR-capable clients
outside the first-class Claude/Cowork and Codex paths, generated from one
maintained connection definition. With `ppat_` API tokens removed (PR #42,
see the spec's 2026-07-09 banner), there is no advanced bearer-token fallback:
clients without a supported OAuth discovery/DCR path are honestly marked
unsupported until they gain it.

**Acceptance criteria**

- [ ] Given a generic MCP client with a supported Proofplane DCR or manual
  registration path, when the user follows OAuth guidance, then the hosted MCP
  URL is used without a Proofplane-issued client ID or local client allowlist.
- [ ] Given a generic MCP client without a supported OAuth/DCR path, when it is
  selected, then Proofplane states it cannot connect yet rather than offering a
  removed API-token setup or an unsupported OAuth promise.
- [ ] Given Claude/Cowork or Codex setup, when generic guidance changes, then
  their first-class distribution artifacts are unchanged.

**Tasks**

- [ ] Define a versioned source for generic connection metadata.
- [ ] Document production DCR controls for abuse prevention, cleanup
  ownership, and monitoring.
- [ ] Add a client capability/support matrix with last-verified metadata,
  marking non-OAuth clients unsupported.
- [ ] Validate generated snippets in tests or release checks.
- [ ] Document update ownership when a client changes its configuration format.

**Notes**

- Cursor and VS Code distribution are excluded.
- 2026-06-29: Scope narrowed with the spec to remove client-specific
  distribution.
- 2026-07-08: Production Proofplane Dynamic Client Registration requires abuse
  prevention, cleanup ownership, and monitoring.
- 2026-07-09: `ppat_` API tokens were removed in PR #42, so this ticket no
  longer has an advanced bearer-token fallback to document. Non-OAuth/DCR
  clients are unsupported until an unattended-credential replacement is
  designed.
