# 003 — Guide Tool And Depth Docs

**Status:** Done · **Depends on:** none (align topics with 001) · **Spec:** [spec.md](../spec.md#depth-docs-tool--resource-progressive-disclosure)

**Summary** — Author the curated topic docs and expose them through a
`get_proofplane_guide(topic)` tool, giving the model a reliable on-demand depth
channel that works in both Codex and Claude.

**Acceptance criteria**

- [x] Given a known topic, when `get_proofplane_guide(topic)` is called, then it
  returns that topic's markdown content.
- [x] Given no topic or an unknown topic, when the tool is called, then it
  returns the index of valid topics rather than an error.
- [x] Given the topic set, when listed, then it is exactly `glossary`,
  `submitting-evidence`, `controls-and-mappings`, `attachments`, and
  `errors-and-not-found`.
- [x] Given the docs, when the server is built, then content is embedded in the
  binary (`include_str!`) from a single topic registry, not read from disk at
  runtime.
- [x] Given the guide tool, when authorization is checked, then it requires only
  a valid connection and exposes no workspace-specific data.
- [x] Given the guide surface is registered, when relevant evidence, attachment,
  and control tool descriptions are listed, then their guide references name
  registered topics; auditor access descriptions have no unrelated pointer.

**Tasks**

- [x] Add `src/mcp/docs/*.md` for each topic and a topic→title→content registry.
- [x] Add the `get_proofplane_guide` tool in its own `#[tool_router]` module.
- [x] Implement unknown/empty-topic index behavior.
- [x] Add guide-tool references to relevant evidence, attachment, and control
  descriptions without adding unrelated pointers to auditor access tools.
- [x] Add unit tests: known topic returns content, unknown returns index, all
  registered topics resolve, and description references target registered topics.
- [x] Run `make check`.

**Notes**

- The registry is the single source shared with ticket 004's resource surface.
- Keep docs per-*topic*, not per-tool (spec "Topic Inventory").
- Reconciled the spec with the shipped response envelope, authorization rule,
  description-pointer mapping, instruction reference, and resource deferral.
