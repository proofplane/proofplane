# 002 - Compliance Read Tools

**Status:** Todo · **Depends on:** 001, evidence-lifecycle-completion/002, evidence-lifecycle-completion/004, trusted-compliance-reads/004 · **Spec:** [spec.md](../spec.md#mvp-tools)

**Summary** - Add the MVP read tools for requests, selectively detailed
submissions, human attachment download grants, controls, and compact packet
previews.

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
- [ ] Given a latest-submission read, when it succeeds, then the bounded summary
  may be returned but the description is absent.
- [ ] Given a submission read by ID, when it succeeds, then its optional summary
  and description are returned without duplicated explanatory prose.
- [ ] Given a packet preview, when it succeeds, then summaries and descriptions
  are absent from the aggregate result.

**Tasks**

- [ ] Add request, submission, download-grant, control, and mapping read tools.
- [ ] Add the packet-preview tool.
- [ ] Map domain problems to stable MCP problem codes.
- [ ] Add representative integration tests for every read family.

**Notes**

- The spec was revised on 2026-06-20 to defer source material and disclose
  submission context only through focused reads.
