# MCP Agent Guidance Epic

Help an agent learn **how to use Proofplane** the moment it connects to the MCP
server — domain concepts, vocabulary, workflow order, and constraints — without
the human writing a system prompt by hand. The core principle is **teach on
connect, disclose depth on demand**: return minimal orienting context during
initialization for clients that use it, keep portable guidance in tool
descriptions, and let the agent pull deeper docs only when needed (a
`get_proofplane_guide` tool and matching `proofplane://docs/{topic}` resources).

Full rationale, channel tradeoffs, the Codex 512-character constraint, the topic
inventory, and the instructions draft live in [spec.md](./spec.md) — the single
source of technical depth. Tickets below are lean handoff units that link into
it.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Server Instructions Manual](./tickets/001-server-instructions-manual.md) | Done | Added initialization guidance with explicitly client-dependent model-context handling. |
| 002. [Tool Description Pass](./tickets/002-tool-description-pass.md) | Done | Rewrote all 20 descriptions and added exact router and authenticated discovery coverage. |
| 003. [Guide Tool And Depth Docs](./tickets/003-guide-tool-and-depth-docs.md) | Todo | Embed the curated topic docs and expose them via `get_proofplane_guide(topic)`. |
| 004. [Doc Resources Surface](./tickets/004-doc-resources-surface.md) | Todo | Expose the same docs as `proofplane://docs/{topic}` resources and enable the resources capability. |

## Sequencing

- **001** and **002** are independent and can be built in parallel; both are
  foundational. Server instructions add orientation where clients use them,
  while tool descriptions provide the portable per-tool layer.
- **003** defines the canonical curated topic docs and the model-pull channel.
  After that surface exists, it adds guide-tool references to relevant evidence,
  attachment, and control descriptions; auditor tools receive no unrelated
  guide pointer. Its guide references can also be added to the instructions.
- **004** depends on **003** because it reuses the same embedded doc content;
  it adds the resource surface and the resources capability, after which the
  instructions can also reference resources.
- Prompts are out of scope for this iteration (see spec "Out Of Scope").
