# 004 - MCP First-Run Experience

**Status:** Todo · **Depends on:** 003, mcp-server/002, mcp-server/003, trusted-compliance-reads/004 · **Spec:** [spec.md](../spec.md#agent-first-success-path)

**Summary** - Help a workspace owner get a useful answer from their own AI agent
through suggested prompts and real MCP tool calls, without a Proofplane CRUD UI.

**Acceptance criteria**

- [ ] Given a configured MCP client, when the user asks what remains for SOC 2,
  then the agent can inspect the sandbox and answer from real Proofplane data.
- [ ] Given the suggested write prompt, when the agent creates and maps an
  Evidence Request, then real sandbox records are changed through MCP.
- [ ] Given the packet prompt, when the agent requests a preview, then the tool
  result includes provenance and explicit missing-evidence gaps.
- [ ] Given the browser setup surface, when this ships, then it contains no
  control, Evidence Request, mapping, source-material, or packet-editing forms.
- [ ] Given unsupported, denied, or invalid tool input, when the agent calls
  Proofplane, then structured MCP errors allow it to explain the failure.

**Tasks**

- [ ] Add suggested read, write, and packet prompts with plain-language context.
- [ ] Add MCP fixtures covering the suggested prompt tool sequences.
- [ ] Add setup guidance for reconnecting and rotating credentials.
- [ ] Add browser smoke coverage from sandbox CTA through MCP setup instructions.
- [ ] Add integration coverage for the prompt-driven MCP read/write outcomes.

**Notes**

- Interface behavior is specified in [ux.md](../ux.md#suggested-prompts).
