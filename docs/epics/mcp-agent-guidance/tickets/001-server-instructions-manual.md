# 001 — Server Instructions Manual

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#server-instructions-backbone)

**Summary** — Add the backbone `instructions` field to the MCP server so every
connecting agent reads a concise Proofplane "user manual" — domain, core loop,
and key constraints — on connect. _As an operator, I want the model to
understand Proofplane without me hand-writing a system prompt._

**Acceptance criteria**

- [ ] Given a client initializes the MCP server, when it reads the returned
  `ServerInfo`, then a non-empty `instructions` string is present.
- [ ] Given the instructions, when only the first 512 characters are read
  (Codex's window), then they self-containedly state the domain, the core loop,
  and the one-workspace constraint.
- [ ] Given the instructions, when inspected, then they describe cross-tool
  workflow and constraints and do not restate individual tool descriptions or
  add marketing.
- [ ] Given every guide topic named in the instructions, when checked, then each
  names a topic that ticket 003 registers (no dangling topic references).
- [ ] Given existing tool behavior, when instructions are added, then tool
  results and MCP runtime authorization are unchanged.

**Tasks**

- [ ] Add `.with_instructions(...)` in `get_info` (`src/mcp/server/mod.rs`).
- [ ] Write the manual from the spec draft, front-loading the 512-char lead.
- [ ] Add unit tests for non-emptiness, the 512-char lead content, and topic
  references resolving.
- [ ] Run `make check`.

**Notes**

- Wording and the 512-char lead are anchored by the spec's "Server Draft:
  Instructions"; refine there if the manual changes.
- The topic list in the instructions must stay in sync with ticket 003's
  registry — land them together or sequence 001 first.
