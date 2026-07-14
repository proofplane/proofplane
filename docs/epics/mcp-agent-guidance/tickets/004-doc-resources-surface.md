# 004 — Doc Resources Surface

**Status:** Done · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#depth-docs-tool--resource-progressive-disclosure)

**Summary** — Expose the same curated topic docs as `proofplane://docs/{topic}`
MCP resources and advertise the resources capability, so clients and humans that
surface resources (Codex, MCP Inspector) can browse them — reusing ticket 003's
embedded content.

**Acceptance criteria**

- [x] Given the server capabilities, when a client initializes, then resources
  are advertised alongside tools.
- [x] Given `list_resources`, when called, then it enumerates exactly the
  registered topics as `proofplane://docs/{topic}` with stable URIs and titles.
- [x] Given `read_resource` for a `proofplane://docs/{topic}` URI, when the
  topic is registered, then it returns that topic's markdown — identical to
  `get_proofplane_guide(topic)`.
- [x] Given an unregistered or malformed docs URI, when read, then it fails
  closed with a resource-not-found error rather than empty content.
- [x] Given the tools capability, when resources are added, then existing tool
  behavior and runtime authorization are unchanged.

**Tasks**

- [x] Enable resources in `ServerCapabilities` (`get_info`).
- [x] Implement `list_resources` / `read_resource` over the ticket 003 registry.
- [x] Add unit tests: enumeration matches the registry, tool/resource content
  parity, and not-found on bad URIs.
- [x] Run `make check`.

**Notes**

- Content and the topic registry come from ticket 003; this ticket adds only the
  resource surface, so the two channels never diverge.
- The shipped resource wire contract and instruction revision are recorded in
  the spec's depth-docs section and 2026-07-14 revision.
