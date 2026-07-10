# MCP Agent Guidance Spec

## Goal

When an agent connects to the Proofplane MCP server, it should quickly learn
**how to use Proofplane** — the domain concepts, the vocabulary, the core
workflow, and the constraints — without the human operator hand-writing a
system prompt. Observed problem: through the Codex harness, the model can call
tools but does not reliably understand the domain (evidence request vs.
submission vs. control vs. attachment grant) or the correct workflow order.

The core principle is **teach on connect, disclose depth on demand**: put the
minimum orienting context where every client reliably reads it, and let the
agent pull deeper documentation only when it needs it.

## Channels And Why We Use Each

MCP exposes three primitives (tools, resources, prompts) plus one community
pattern (progressive disclosure). They solve different parts of the problem, so
this epic composes them rather than picking one.

| Channel | Reliability on connect | Role in this epic |
| --- | --- | --- |
| **Server `instructions`** | Injected into the system prompt on connect; read by Codex and Claude | **Backbone.** The orienting "user manual." |
| **Tool descriptions** | Sent on every `list_tools`; always in context | **Per-tool semantics.** One domain sentence each. |
| **`get_proofplane_guide` tool** | Tool-calling is the one channel every client lets the *model* invoke autonomously | **Depth (model pull).** On-demand topic docs. |
| **`proofplane://docs/{topic}` resources** | Pull-based; surfacing varies by client | **Depth (client/human surface).** Same content, idiomatic form. |
| **Prompts** | User-invoked, not auto-read | **Out of scope this iteration** (see below). |

### Server instructions (backbone)

Guidance from the official
[server-instructions post](https://blog.modelcontextprotocol.io/posts/2025-11-03-using-server-instructions/):
instructions are a *user manual for the server*, not a place to restate tool
descriptions, add marketing, or change model personality. Keep them concise —
the more text, the less reliably a model follows any of it — and focus on
**cross-tool relationships, workflow order, domain vocabulary, and
constraints** the tools cannot convey.

**Codex-specific constraint:** Codex leans on the **first ~512 characters** of
the instructions when deciding how to use the server
([OpenAI Codex MCP docs](https://developers.openai.com/codex/mcp)). The lead of
the instructions must therefore be self-contained: the domain in one line, the
core loop, and the single most important constraint, before any deeper prose.

The server currently sets **no** instructions (`get_info` returns
`ServerInfo::new(caps).with_server_info(...)` only). This is the highest-ROI
change in the epic.

### Tool descriptions (per-tool)

Descriptions are always in context, so they are the most reliable per-tool
channel. Today they are terse (`"List controls in a workspace."`) and one is
**wrong**: `create_evidence_submission` advertises *"return REST-only
attachment upload instructions,"* but the REST data-plane was removed in PR #42
and attachments now flow through `manage_evidence_submission_attachment`. Each
description gets one sentence of domain semantics; write tools end with a
pointer to the relevant guide topic (e.g. `See guide: submitting-evidence`).

### Depth docs: tool + resource (progressive disclosure)

[Progressive disclosure](https://www.solo.io/blog/mcp-progressive-disclosure)
is a community pattern (not in the spec): keep the initial context small and
fetch full docs on demand. The portability trap for our two target clients:
**resources are surfaced differently by Codex and Claude.** Several clients,
Claude among them, surface resources for the *human* to attach as context and
do not let the *model* autonomously enumerate and read them. Depth kept only in
resources could therefore go unused in Claude.

Decision: expose each depth doc through **both** channels, backed by the same
markdown content:

- **`get_proofplane_guide(topic)` tool** — guarantees the *model* can pull
  depth in both Claude and Codex, because tool-calling is universally
  model-invocable. This is what actually makes progressive disclosure work
  across clients.
- **`proofplane://docs/{topic}` resources** — spec-idiomatic and browsable by
  clients/humans that surface resources well (e.g. Codex, MCP Inspector).

Content is embedded in the binary (`include_str!` over markdown files under
`src/mcp/docs/`) so docs ship with the server, are unit-testable, and cannot
drift from the deployed build. The tool and the resource handler read the same
embedded strings; there is exactly one source per topic.

### Prompts (deferred)

MCP prompts are reusable, parameterized templates, but they are **user-invoked**
(a slash command the human runs), not read automatically on connect, so they do
not address "orient the model when it connects." They are a good later addition
for canned workflows (e.g. "collect SOC 2 evidence for control X") and are
deferred to a future iteration.

## Topic Inventory

The depth docs are a **small, curated topic set**, not one-per-tool (17 tools
would mean 17 drifting docs). Initial topics:

| Topic (`get_proofplane_guide` arg / resource path) | Content |
| --- | --- |
| `glossary` | The domain vocabulary, expanded with examples. |
| `submitting-evidence` | Request → submission → attachment, end to end. |
| `controls-and-mappings` | Frameworks, requirements, controls, and mappings. |
| `attachments` | Why attachment bytes move through a human browser grant, and the grant lifecycle. |
| `errors-and-not-found` | How the server conceals workspace/permission failures as not-found, so the model does not misread them. |

`get_proofplane_guide` with no/unknown topic returns the list of valid topics
(a lightweight index), so the model can discover what is available.

## Domain Model (source for the instructions and glossary)

```
Framework (global)
  └─ Requirement (global)
        └─ Control (workspace)
              └─ Control mapping (Control ↔ Evidence request)
Evidence request (workspace, has collection_instructions + due date)
  └─ Evidence submission (records proof, carries agent provenance)
        └─ Attachment (uploaded/downloaded via short-lived human browser grant)
```

Core loop the instructions must teach:

1. **Find what's needed** — `list_evidence_requests` /
   `list_due_evidence_requests`; read each request's `collection_instructions`.
2. **Record proof** — `create_evidence_submission` against an evidence request.
3. **Attach files** — `manage_evidence_submission_attachment` returns a
   short-lived browser URL a human uploads through; **file bytes never pass
   through MCP or the model.**
4. **Controls** — controls define what must be proven; map them to evidence
   requests with `map_evidence_request_to_control`.

Key constraints to state explicitly:

- exactly **one workspace** per connection (see the
  [Agent Connector Onboarding](../agent-connector-onboarding/spec.md)
  2026-07-09 decision banner);
- **attachments flow through human browser sessions, never MCP**;
- submissions capture agent-connection provenance; and
- workspace/permission failures surface as **not-found**, not as explicit
  authorization errors.

## Server Draft: Instructions

Front-loaded so the first ~512 characters are self-contained for Codex:

> Proofplane is a SOC 2 / compliance evidence platform. You operate inside one
> workspace. Core loop: (1) find what's needed — `list_evidence_requests` /
> `list_due_evidence_requests`; (2) record proof — `create_evidence_submission`
> against an evidence request; (3) attach files —
> `manage_evidence_submission_attachment` returns a short-lived browser URL a
> human uploads through (file bytes never pass through you or the model).
> Always read an evidence request's `collection_instructions` before
> submitting. Controls define what must be proven; map them to evidence
> requests.
>
> Domain model: Framework → Requirement (global) → Control (workspace) → Control
> mapping (Control ↔ Evidence request) → Evidence request → Evidence submission
> → Attachment (via browser grant). Constraints: exactly one workspace per
> connection; attachments move through human browser sessions, never MCP;
> submissions capture agent provenance; workspace/permission failures surface
> as not-found. For deeper detail on any concept or workflow, call
> `get_proofplane_guide` (topics: glossary, submitting-evidence,
> controls-and-mappings, attachments, errors-and-not-found) or read the
> `proofplane://docs/{topic}` resources.

Exact wording is finalized in ticket 001; this draft anchors length and the
512-character lead.

## Code Touchpoints

- `src/mcp/server/mod.rs` (`get_info`): add `.with_instructions(...)`; extend
  `ServerCapabilities` to enable resources alongside tools.
- New `src/mcp/docs/*.md` embedded via `include_str!`; a single topic registry
  (topic → title → markdown) shared by the tool and the resource handler.
- New `get_proofplane_guide` tool (its own `#[tool_router]` module under
  `src/mcp/server/`).
- Resource handler implementing `list_resources` / `read_resource` for
  `proofplane://docs/{topic}`.
- Description edits across the 17 existing tools (fix the stale
  `create_evidence_submission` text first).

## Testing

- Unit: instructions are non-empty, the 512-char lead names the core loop and
  the one-workspace constraint, and every topic referenced in the instructions
  resolves to real content.
- Unit: every registered topic is reachable through **both**
  `get_proofplane_guide` and `read_resource`, and the two return identical
  content; unknown topics return the topic index, not an error dump.
- Unit: `list_resources` enumerates exactly the registered topics with stable
  URIs.
- Contract: no tool description references REST or `ppat_`; write-tool
  descriptions point at a topic that exists.
- Coexistence: existing tool behavior and MCP runtime authorization are
  unchanged; adding instructions/resources does not alter tool results.

## Out Of Scope

- User-invoked **prompts** (deferred to a later iteration).
- Per-tool doc resources (curated per-*topic* docs instead).
- Dynamic/remote doc content — docs are embedded and versioned with the binary.

## Reference Material

- [Server Instructions: Giving LLMs a user manual for your server](https://blog.modelcontextprotocol.io/posts/2025-11-03-using-server-instructions/)
- [OpenAI Codex — Model Context Protocol](https://developers.openai.com/codex/mcp)
- [MCP specification (2025-11-25)](https://modelcontextprotocol.io/specification/2025-11-25)
- [MCP Progressive Disclosure](https://www.solo.io/blog/mcp-progressive-disclosure)

## Revisions

- 2026-07-10: Initial spec. Chose backbone (server instructions) + depth docs
  exposed through both a `get_proofplane_guide` tool and
  `proofplane://docs/{topic}` resources, tuned for Codex and Claude. Prompts
  deferred.
