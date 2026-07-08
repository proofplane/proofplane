# 007 - Codex Direct MCP Integration

**Status:** Done · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#codex)

**Summary** - Validate Codex through direct remote MCP setup using Proofplane
Dynamic Client Registration and Proofplane workspace consent. This replaces
the earlier Codex plugin plan; no `proofplane-soc2` plugin or marketplace
package is required for this epic.

**Acceptance criteria**

- [x] Given direct Codex MCP setup, when Codex reads Proofplane Protected
  Resource Metadata, then it discovers Proofplane as the authorization server.
- [x] Given no existing Codex OAuth client, when Codex starts login, then
  Proofplane Dynamic Client Registration creates the local public client.
- [x] Given no existing authorization, when Codex login runs, then the user is
  sent through Proofplane OAuth, upstream Auth0 login, and Proofplane workspace
  consent rather than asked for a Proofplane API token.
- [x] Given the Proofplane workspace step is approved, when Proofplane issues
  the authorization code, then Codex receives the authorization redirect and
  completes the login after the app is restarted if its local callback listener
  was stale.
- [x] Given the earlier plugin plan, when the direct MCP path is validated, then
  plugin packaging is removed from this epic rather than carried as backlog.

**Tasks**

- [x] Validate Codex Protected Resource Metadata discovery.
- [x] Validate Codex Proofplane authorization-server discovery and DCR.
- [x] Validate Proofplane authorization-code and PKCE token exchange.
- [x] Validate Proofplane workspace consent and PASETO token use.
- [x] Record that restarting Codex cleared the stale local callback listener
  failure.
- [x] Remove Codex plugin packaging from the epic scope.

**Notes**

- 2026-07-07: Codex `0.142.5` validates the dev-first OAuth path through
  Protected Resource Metadata, Proofplane authorization-server metadata,
  Proofplane Dynamic Client Registration, Proofplane workspace consent, and
  Proofplane token exchange.
- 2026-07-07: The earlier `proofplane-soc2` plugin plan is removed from this
  epic. Direct Codex MCP setup is the supported Codex integration path.
- 2026-06-29: Spec now distinguishes documented Codex MCP OAuth via
  `codex mcp login` from the observed desktop custom-MCP form, which exposes
  bearer-token configuration rather than an inline OAuth connect action.
