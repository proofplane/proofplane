# 002 — Tool Description Pass

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#tool-descriptions-per-tool)

**Summary** — Fix the stale `create_evidence_submission` description and give each
of the 20 MCP tools one concise sentence of domain semantics, so the portable
per-tool channel teaches accurately.

**Acceptance criteria**

- [x] Given the `create_evidence_submission` tool, when its description is read,
  then it describes the current human-browser attachment flow and does **not**
  mention REST or "REST-only".
- [x] Given any tool description, when scanned, then no description references
  REST or `ppat_` (both removed in PR #42).
- [x] Given guide tools and resources are not yet registered, when descriptions
  are listed, then none references an unavailable guide tool, topic, or resource.
- [x] Given a client lists tools, when descriptions are compared to behavior,
  then each description matches the tool's actual inputs and effect.

**Tasks**

- [x] Rewrite the `create_evidence_submission` description (remove REST).
- [x] Add one concise domain sentence to each of the 20 tool descriptions.
- [x] Add router coverage for the exact tool inventory and descriptions, sentence
  shape, stale terminology, internal boundary language, and unavailable guides.
- [x] Extend authenticated discovery coverage for descriptions while preserving
  existing tool-name and schema assertions.
- [x] Run `make check`.

**Notes**

- The stale text is `src/mcp/server/evidence_submissions.rs:32`.
- Ticket 003 owns adding guide-tool references to relevant evidence, attachment,
  and control descriptions after it registers the guide surface. Auditor tools
  receive no unrelated guide pointer.
