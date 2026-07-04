# 005 - Codex Compliance Plugin

**Status:** Todo · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#codex)

**Summary** - Package Proofplane's hosted MCP connection with focused SOC 2
skills and safe first-run guidance so a user can add one Codex plugin instead
of configuring a raw MCP server. Validate the Codex app/plugin OAuth path
before presenting it as no-token onboarding, because the current desktop
custom-MCP form appears to require bearer-token configuration.

**Acceptance criteria**

- [ ] Given the Proofplane plugin in Codex, when a user installs it, then the
  MCP server and bundled compliance workflows become available through the
  native plugin experience under the Proofplane-owned `proofplane-soc2`
  package.
- [ ] Given no existing authorization, when the plugin is first used in a
  Codex surface that supports MCP OAuth, then the user is sent through Auth0
  and Proofplane workspace consent rather than asked for a token.
- [ ] Given the Codex desktop custom-MCP form remains bearer-token oriented,
  when a user chooses direct custom-MCP setup, then Proofplane labels it as
  advanced setup rather than the default non-technical path.
- [ ] Given a plugin package or marketplace entry with embedded customer
  credentials, when validated, then publication is rejected.
- [ ] Given direct Codex MCP configuration already in use, when the plugin
  ships, then that advanced connection path remains supported.
- [ ] Given the 24-hour access token expires, when Codex next uses the MCP
  server, then automatic reauthorization or its reconnect path is verified and
  documented.

**Tasks**

- [ ] Define the initial compliance workflows and server instructions.
- [ ] Create the Proofplane-owned `proofplane-soc2` plugin package and
  marketplace entry for preview distribution.
- [ ] Verify whether Codex app plugin installation can trigger MCP OAuth
  against the Auth0 issuer without manual CLI/config work.
- [ ] Create the plugin manifest, MCP declaration, skills, and first prompts.
- [ ] Add package validation and credential-leak checks.
- [ ] Exercise install, authorization or documented fallback, tool use,
  token expiry, reconnect, disable, and uninstall in the Codex app.
- [ ] Prepare marketplace metadata and workspace-sharing documentation.
- [ ] Add a release checklist for plugin and MCP compatibility.

**Notes**

- 2026-07-02: The spec now validates plugin-led OAuth directly against Auth0
  rather than a Proofplane authorization facade.
- 2026-07-02: The plugin release gate includes access-token expiry because the
  initial release does not request `offline_access`.
- 2026-06-29: Spec now distinguishes documented Codex MCP OAuth via
  `codex mcp login` from the observed desktop custom-MCP form, which exposes
  bearer-token configuration rather than an inline OAuth connect action.
- 2026-06-29: Spec now fixes initial plugin ownership and name:
  `proofplane-soc2`, distributed first through a Proofplane-controlled
  marketplace.
