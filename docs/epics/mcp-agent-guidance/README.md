# MCP Agent Guidance Epic

Help an agent learn **how to use Proofplane** the moment it connects to the MCP
server — domain concepts, vocabulary, workflow order, and constraints — without
the human writing a system prompt by hand. The core principle is **teach on
connect, disclose depth on demand**: put minimal orienting context where every
client reliably reads it (server instructions + tool descriptions), and let the
agent pull deeper docs only when it needs them (a `get_proofplane_guide` tool
and matching `proofplane://docs/{topic}` resources).

Full rationale, channel tradeoffs, the Codex 512-character constraint, the topic
inventory, and the instructions draft live in [spec.md](./spec.md) — the single
source of technical depth. Tickets below are lean handoff units that link into
it.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Server Instructions Manual](./tickets/001-server-instructions-manual.md) | Todo | Add the backbone `instructions` manual with a self-contained 512-char lead for Codex. |
| 002. [Tool Description Pass](./tickets/002-tool-description-pass.md) | Todo | Fix the stale `create_evidence_submission` REST text; add one domain sentence + guide pointer per tool. |
| 003. [Guide Tool And Depth Docs](./tickets/003-guide-tool-and-depth-docs.md) | Todo | Embed the curated topic docs and expose them via `get_proofplane_guide(topic)`. |
| 004. [Doc Resources Surface](./tickets/004-doc-resources-surface.md) | Todo | Expose the same docs as `proofplane://docs/{topic}` resources and enable the resources capability. |

## Sequencing

- **001** and **002** are independent and can be built in parallel; both are
  foundational (they are the channels both Codex and Claude read reliably on
  connect) and ship value on their own.
- **003** defines the curated topic docs and the model-pull channel. Its topic
  set must match the topics referenced by 001's instructions; sequence 001
  before finalizing 003's list, or land them together.
- **004** depends on **003** because it reuses the same embedded doc content;
  it adds the resource surface and the resources capability.
- Prompts are out of scope for this iteration (see spec "Out Of Scope").
