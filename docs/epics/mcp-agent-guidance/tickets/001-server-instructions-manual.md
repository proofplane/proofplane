# 001 — Server Instructions Manual

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#server-instructions-backbone)

**Summary** — Add the backbone `instructions` field to the MCP server so clients
that use initialization guidance can give the model a concise Proofplane "user
manual" — domain, core loop, and key constraints. _As an operator, I want the
model to understand Proofplane without me hand-writing a system prompt._

**Acceptance criteria**

- [x] Given a client initializes the MCP server, when it reads the returned
  `ServerInfo`, then a non-empty `instructions` string is present.
- [x] Given the instructions, when only the first 512 characters are read
  (Codex's window), then they self-containedly state the domain, the core loop,
  and that attachments use a human browser while file bytes never pass through
  MCP or the model.
- [x] Given the instructions, when inspected, then they describe cross-tool
  workflow, `collection_instructions`, control mappings, agent provenance, and
  secure browser-URL handoff without restating individual tool descriptions or
  adding marketing.
- [x] Given the instruction text, when inspected, then it contains no internal
  boundary terminology, REST/`ppat_` references, or guide tool/resource
  references for surfaces that have not shipped.
- [x] Given existing tool behavior, when instructions are added, then tool
  results and MCP runtime authorization are unchanged.

**Tasks**

- [x] Add `.with_instructions(...)` in `get_info` (`src/mcp/server/mod.rs`).
- [x] Write the manual from the spec draft, front-loading the 512-char lead.
- [x] Add unit tests for attachment, lead-content, relationship, and exclusion
  contracts; extend the authenticated MCP initialization integration test.
- [x] Run `make check`.

**Notes**

- Wording and the 512-char lead are anchored by the spec's "Server Draft:
  Instructions"; refine there if the manual changes.
- Connection binding remains internal authorization architecture, not agent
  guidance.
- The five future guide topics remain canonical in the spec, but instructions
  reference them only after tickets 003 and 004 provide those surfaces.
- Spec revised 2026-07-13 to record both decisions above.
- Spec revised 2026-07-13 to replace concealed not-found guidance with the
  browser URL's bearer-secret handoff and expiry rule.
- Spec revised 2026-07-13 to clarify that client and surface behavior determines
  whether initialization instructions enter model context.
