# 002 - Compliance Read Tools

**Status:** Todo · **Depends on:** 001, evidence-lifecycle-completion/002, trusted-compliance-reads/004 · **Spec:** [spec.md](../spec.md#mvp-tools)

**Summary** - Add the MVP read tools for requests, submissions, human attachment
download grants, controls, curated source material, and packet previews.

**Acceptance criteria**

- [ ] Given an authorized user API token, when a read tool is called, then it
  returns the same domain records and tenant scope as the equivalent REST
  operation.
- [ ] Given invalid input, when a tool is called, then structured field issues
  identify every invalid field.
- [ ] Given unauthorized or cross-workspace input, when a tool is called, then a
  not-found problem is returned without leaking resource existence.
- [ ] Given a finalized attachment, when a download grant is requested, then the
  tool returns a five-minute HTTPS URL without embedding or fetching
  attachment bytes.
- [ ] Given a download-grant result, when it is returned, then it identifies the
  URL as a bearer secret for human use and excludes the raw token from logs.

**Tasks**

- [ ] Add request, submission, download-grant, control, and mapping read tools.
- [ ] Add source-material and packet-preview tools.
- [ ] Map domain problems to stable MCP problem codes.
- [ ] Add representative integration tests for every read family.
