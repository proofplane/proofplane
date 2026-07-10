# 002 — Tool Description Pass

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#tool-descriptions-per-tool)

**Summary** — Fix the stale `create_evidence_submission` description and give each
of the 17 MCP tools one sentence of domain semantics, so the always-in-context
per-tool channel teaches accurately.

**Acceptance criteria**

- [ ] Given the `create_evidence_submission` tool, when its description is read,
  then it describes the current human-browser attachment flow and does **not**
  mention REST or "REST-only".
- [ ] Given any tool description, when scanned, then no description references
  REST or `ppat_` (both removed in PR #42).
- [ ] Given each write tool, when its description is read, then it ends with a
  pointer to a guide topic (e.g. `See guide: submitting-evidence`) that ticket
  003 registers.
- [ ] Given a client lists tools, when descriptions are compared to behavior,
  then each description matches the tool's actual inputs and effect.

**Tasks**

- [ ] Rewrite the `create_evidence_submission` description (remove REST).
- [ ] Add one domain sentence to each of the 17 tool descriptions.
- [ ] Append `See guide: <topic>` pointers to write-tool descriptions.
- [ ] Add a test asserting no description contains "REST" or "ppat_" and that
  guide pointers reference registered topics.
- [ ] Run `make check`.

**Notes**

- The stale text is `src/mcp/server/evidence_submissions.rs:32`.
- Guide-topic pointers depend on ticket 003's registry; if 003 has not landed,
  keep the pointer text but cover the "topic exists" assertion once 003 lands.
