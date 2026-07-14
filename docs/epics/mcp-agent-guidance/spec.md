# MCP Agent Guidance Spec

## Goal

When an agent connects to the Proofplane MCP server, it should quickly learn
**how to use Proofplane** — the domain concepts, the vocabulary, the core
workflow, and the constraints — without the human operator hand-writing a
system prompt. Observed problem: through the Codex harness, the model can call
tools but does not reliably understand the domain (evidence request vs.
submission vs. control vs. attachment grant) or the correct workflow order.

The core principle is **teach on connect, disclose depth on demand**: return the
minimum orienting context during initialization for supporting clients, keep
portable semantics in tool descriptions, and let the agent pull deeper
documentation only when it needs it.

## Channels And Why We Use Each

MCP exposes three primitives (tools, resources, prompts) plus one community
pattern (progressive disclosure). They solve different parts of the problem, so
this epic composes them rather than picking one.

| Channel | Reliability on connect | Role in this epic |
| --- | --- | --- |
| **Server `instructions`** | Returned during MCP initialization; supporting clients may include it in model context, but treatment varies by client and surface | **Backbone where supported.** The orienting "user manual." |
| **Tool descriptions** | Available through standard tool discovery; clients may preload or search them dynamically | **Portable per-tool semantics.** One domain sentence each. |
| **`get_proofplane_guide` tool** | A callable surface lets the model pull depth in the target clients | **Depth (model pull).** On-demand topic docs. |
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

Before ticket 001, the server set **no** instructions. Ticket 001 adds the
static manual through `ServerInfo::with_instructions(...)`; this is the
highest-ROI change in the epic.

The connection's authorization scope remains an internal server concern. The
manual does not teach agents to select or manage that scope, and it does not
expose internal boundary terminology. Within the 512-character lead, the most
important constraint is instead the attachment data path: a human uses the
browser flow, and file bytes never pass through MCP or the model.

### Tool descriptions (per-tool)

Descriptions travel with standard tool definitions, making them the most
portable per-tool channel even though clients may load them up front or through
dynamic tool search. Today they are terse (`"List controls in a workspace."`)
and one is **wrong**: `create_evidence_submission` advertises *"return REST-only
attachment upload instructions,"* but the REST data-plane was removed in PR #42
and attachments now flow through `manage_evidence_submission_attachment`.
Ticket 002 gives each of the 20 current tools one concise sentence of domain
semantics and does not reference guide or resource surfaces that are not yet
registered. After registering the guide surface, ticket 003 owns adding
guide-tool references to relevant descriptions: evidence request and submission
tools point to `submitting-evidence`, the attachment tool points to
`attachments`, and framework, control, and mapping tools point to
`controls-and-mappings`. Auditor access tools receive no unrelated guide
pointer. Each description remains one sentence.

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

Ticket 003 ships the tool channel and shared registry. Ticket 004 remains
responsible for the resource channel and for enabling the MCP resources
capability; no resource URI is advertised before then.

Content is embedded in the binary (`include_str!` over markdown files under
`src/mcp/docs/`) so docs ship with the server, are unit-testable, and cannot
drift from the deployed build. The tool and the resource handler read the same
embedded strings; there is exactly one source per topic.

The guide tool takes one optional string, `topic`. It trims surrounding
whitespace and exact-matches the lowercase slug. Missing, blank,
case-mismatched, and unknown values all return the deterministic index without
echoing the rejected value. Every response uses the same envelope:

- `topic: Option<String>`;
- `title: String`;
- `markdown: String`; and
- `topics: Vec<{ topic, title }>`.

A known topic returns its slug, title, and embedded Markdown with an empty
`topics` list. The index returns `topic: null`, title `Proofplane guide topics`,
Markdown asking the caller to select a listed topic, and all registry summaries
in canonical order.

Because the guide returns static content, authorization validates only the
middleware-provided agent connection and request context. It requires no
`WorkspacePermission`, performs no persistence, exposes no connection- or
workspace-specific response fields, and emits no audit event.

The resource channel is static and non-paginated. `resources/list` returns the
five registry entries in canonical topic order with URI
`proofplane://docs/{topic}`, `name` equal to the topic slug, the registry title,
and MIME type `text/markdown`; it returns no `nextCursor`. `resources/read`
accepts only an exact, case-sensitive canonical URI and returns one text content
block with the same URI, MIME type, and registry Markdown. Unknown topics and
all malformed variants fail with resource-not-found (`-32002`) using the
existing `not_found` problem envelope. The server advertises `resources: {}`
alongside `tools: {}` and supports neither templates, subscriptions,
list-change notifications, annotations, descriptions, nor sizes. Like the
guide tool, list and read validate only the authenticated agent connection,
require no `WorkspacePermission`, perform no persistence, and emit no audit
event.

### Prompts (deferred)

MCP prompts are reusable, parameterized templates, but they are **user-invoked**
(a slash command the human runs), not read automatically on connect, so they do
not address "orient the model when it connects." They are a good later addition
for canned workflows (e.g. "collect SOC 2 evidence for control X") and are
deferred to a future iteration.

## Topic Inventory

The depth docs are a **small, curated topic set**, not one-per-tool (20 tools
would mean 20 drifting docs). Initial topics:

| Topic (`get_proofplane_guide` arg / resource path) | Title | Content |
| --- | --- | --- |
| `glossary` | Proofplane Glossary | The domain vocabulary, expanded with examples. |
| `submitting-evidence` | Submitting Evidence | Request → submission → attachment, end to end. |
| `controls-and-mappings` | Controls and Mappings | Frameworks, requirements, controls, and mappings. |
| `attachments` | Attachments | Why attachment bytes move through a human browser grant, and the grant lifecycle. |
| `errors-and-not-found` | Errors and Not Found | How the server conceals workspace/permission failures as not-found, so the model does not misread them. |

`get_proofplane_guide` with no/unknown topic returns the list of valid topics
(a lightweight index), so the model can discover what is available.

## Domain Model (source for the instructions and glossary)

```
Framework (global)
  └─ Requirement (global)
        └─ Control
              └─ Control mapping (Control ↔ Evidence request)
Evidence request (has collection_instructions + due date)
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

- **attachments flow through human browser sessions, never MCP**;
- attachment browser URLs are bearer secrets shared only with the human
  managing the attachment before expiry; and
- submissions capture agent-connection provenance.

The server still binds each connection to one authorization boundary as
described in the
[Agent Connector Onboarding spec](../agent-connector-onboarding/spec.md), but
that is internal authorization architecture rather than agent guidance.

## Server Draft: Instructions

Front-loaded so the first ~512 characters are self-contained for Codex:

> Proofplane manages SOC 2 and compliance evidence. Core workflow: first, find
> evidence requests with `list_evidence_requests` or
> `list_due_evidence_requests` and read `collection_instructions`; second,
> create an evidence submission for the request with
> `create_evidence_submission`; third, use
> `manage_evidence_submission_attachment` to get a short-lived human browser
> flow for attachments. A human uploads files there; file bytes never pass
> through MCP or the model.
>
> Frameworks contain requirements, requirements are satisfied by controls, and
> control mappings link controls to evidence requests. Each evidence request
> can have submissions, and each submission can have attachments. Controls
> define what must be proven, so review their mappings when deciding which proof
> satisfies a request. Submissions record the connected agent's provenance.
> Treat the browser URL as a bearer secret and share it only with the human
> managing the attachment before it expires.
>
> Call `get_proofplane_guide` without a topic to see its topic index.

Exact wording is finalized in ticket 001; this draft anchors length and the
512-character lead. Ticket 003 adds guide discovery after that protected lead;
the instructions must not reference resources until ticket 004 implements that
surface.

## Code Touchpoints

- `src/mcp/server/mod.rs` (`get_info`): add `.with_instructions(...)`; extend
  `ServerCapabilities` to enable resources alongside tools.
- New `src/mcp/docs/*.md` embedded via `include_str!`; a single topic registry
  (topic → title → markdown) shared by the tool and the resource handler.
- New `get_proofplane_guide` tool (its own `#[tool_router]` module under
  `src/mcp/server/`).
- Resource handler implementing `list_resources` / `read_resource` for
  `proofplane://docs/{topic}`.
- Description edits across the 20 existing tools (fix the stale
  `create_evidence_submission` text first).

## Testing

- Unit: instructions are non-empty; the 512-char lead names the domain, all
  three stages of the core loop, and the human-browser/file-byte constraint;
  and the full manual covers relationships, mappings, provenance, and secure
  browser-URL handoff.
- Unit: the registry has exactly the five ordered, uniquely named topics with
  non-empty titles and embedded Markdown; every registered guide topic resolves,
  while missing, blank, case-mismatched, and unknown topics return the index.
- Integration: a valid connection with zero permissions can call the guide,
  receives no connection- or workspace-specific fields, and emits no audit
  event.
- Ticket 004 extends coverage so every registered topic is reachable through
  `read_resource` with identical content and `list_resources` enumerates stable
  URIs.
- Contract: ticket 002 descriptions do not reference REST, `ppat_`, internal
  authorization boundaries, or unavailable guide/resource surfaces. Once
  ticket 003 registers the guide tool, guide references in relevant evidence,
  attachment, and control descriptions point at topics that exist.
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
- 2026-07-13: Replaced agent-facing boundary guidance in the 512-character lead
  with the browser-only attachment constraint. Kept connection binding as
  internal authorization architecture and deferred guide/resource references
  until their surfaces ship in tickets 003 and 004.
- 2026-07-13: Replaced concealed not-found guidance with an actionable rule to
  treat attachment browser URLs as bearer secrets and share them only with the
  intended human before expiry.
- 2026-07-13: Clarified that MCP returns server instructions during
  initialization but does not require clients to place them in model context;
  tool descriptions remain the portable model-facing guidance layer.
- 2026-07-13: Corrected the current tool inventory from 17 to 20 and assigned
  guide-tool description references to ticket 003, after that surface exists;
  auditor access descriptions remain focused on auditor access.
- 2026-07-13: Defined the shipped guide response envelope, lowercase
  exact-match/index fallback behavior, connection-only authorization, category
  pointer mapping, and post-lead instruction reference. Resources remain
  deferred to ticket 004.
- 2026-07-14: Shipped the five static documentation resources with exact URI
  matching, canonical non-paginated discovery, `text/markdown` content, and
  connection-only authorization. Enabled the empty resources capability and
  appended protocol-native resource discovery to the full server instructions
  without changing the protected 512-character lead.
