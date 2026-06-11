# 002 - Compliance Read Tools

**Status:** Todo · **Depends on:** 001, trusted-compliance-reads/004 · **Spec:** [spec.md](../spec.md#mvp-tools)

**Summary** - Add the MVP read tools for requests, submissions, controls,
curated source material, and packet previews.

**Acceptance criteria**

- [ ] Given an authorized actor, when a read tool is called, then it returns the
  same domain records and tenant scope as the equivalent REST operation.
- [ ] Given invalid input, when a tool is called, then structured field issues
  identify every invalid field.
- [ ] Given unauthorized or cross-workspace input, when a tool is called, then a
  not-found problem is returned without leaking resource existence.
- [ ] Given attachment or packet bytes are requested, when the tool responds,
  then it returns the authorized REST transfer path rather than embedding bytes.

**Tasks**

- [ ] Add request, submission, control, and mapping read tools.
- [ ] Add source-material and packet-preview tools.
- [ ] Map domain problems to stable MCP problem codes.
- [ ] Add representative integration tests for every read family.
