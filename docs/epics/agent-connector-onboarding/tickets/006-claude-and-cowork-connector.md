# 006 - Claude And Cowork Connector

**Status:** Done · **Depends on:** 003, 004 · **Spec:** [spec.md](../spec.md#claude-and-cowork)

**Summary** - Make the hosted Proofplane MCP endpoint usable from Claude and
Cowork (which share one hosted-surface connector infrastructure) as a custom
connector. The OAuth facade is already client-generic; the only code gap is that
the MCP transport rejected non-loopback `Host` headers. This ticket lands that
fix, provides a preview/ngrok testing path, and validates the real hosted-client
flow through Cowork.

**Acceptance criteria**

- [x] Given a hosted (non-loopback) MCP host in `mcp.allowed_hosts`, when a
  request arrives with that `Host`, then the transport serves it instead of
  returning 403; an empty `allowed_hosts` keeps the loopback-only default.
- [x] Given Claude or Cowork custom-connector setup against the preview URL,
  when the endpoint is added, then the client completes discovery, DCR, Auth0
  login, and workspace consent, and uses only allowed tools. _(Cowork validated
  end-to-end via ngrok, 2026-07-10.)_
- [x] Given a denied or revoked Proofplane grant, when Claude invokes a tool,
  then access fails without falling back to a shared credential.
- [x] Given the 24-hour access token expires, when Claude or Cowork next invokes
  Proofplane, then Proofplane returns `401` and the client must re-run the
  authorization-code flow; this no-refresh-token v1 behavior is documented.
- [x] Given existing loopback clients (Codex, Inspector, local), when the
  allowed-hosts change ships with an empty config list, then their behavior is
  unchanged.

**Tasks**

- [x] Add a configurable `mcp.allowed_hosts` and apply it to the rmcp transport
  only when non-empty (`src/config/*`, `src/mcp/transport.rs`, `src/bin/mcp.rs`);
  config round-trip tests.
- [x] Add `mcp.allowed_hosts` and document the two-tunnel ngrok setup for Codex
  and Cowork in [CONTRIBUTING.md](../../../../CONTRIBUTING.md).
- [x] Validate the live Claude/Cowork custom-connector flow end to end. _(Cowork
  connected via ngrok, 2026-07-10.)_
- [x] Document the accepted no-refresh-token expiry behavior and defer exact
  client UX observation to the refresh-token follow-up.
- [x] Keep the client capability/support matrix, including last-verified
  versions, in ticket 008 rather than duplicating it here.
- [x] Validate Cowork against the existing required `resource` and non-empty
  `scope` authorization fields; retain the strict validation.

**Notes**

- Already satisfied by the generic OAuth facade (verified in code, re-verify
  live): DCR for public clients, the `https://claude.ai/api/mcp/auth_callback`
  redirect, form-urlencoded `/oauth/token`, S256 PKCE, and RFC `invalid_grant`
  errors.
- 2026-07-10: Shipping v1 **without refresh tokens** — the 24h re-consent is an
  accepted, documented limitation. `offline_access` + refresh-token rotation is
  a deferred follow-up (see README).
- 2026-07-10: Directory submission (privacy/support/test-account/tool metadata)
  is out of scope here; at directory scale Claude discourages DCR in favor of
  CIMD or `oauth_anthropic_creds` — a separate follow-up (see README).
- 2026-07-10: Codex and Cowork both work end to end. Exact post-expiry client UX
  remains a non-blocking refresh-token follow-up; the generic support matrix is
  owned by ticket 008.
- 2026-07-07: Codex DCR validation removed the need for Proofplane-side static
  client allowlisting; Claude/Cowork still needs host-specific validation.
