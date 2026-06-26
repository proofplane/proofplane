# 002 - Compliance Read Tools

**Status:** Done · **Depends on:** 001, evidence-lifecycle-completion/002, evidence-lifecycle-completion/004 · **Spec:** [spec.md](../spec.md#core-demo-tools)

**Summary** - Add the core read tools for requests, selectively detailed
submissions, human attachment download grants, controls, and mappings.

**Acceptance criteria**

- [x] Given an authorized user API token, when a read tool is called, then it
  returns the same domain records and tenant scope as the equivalent REST
  operation.
- [x] Given invalid input, when a tool is called, then structured field issues
  identify every invalid field.
- [x] Given unauthorized or cross-workspace input, when a tool is called, then a
  not-found problem is returned without leaking resource existence.
- [x] Given a finalized attachment, when a download grant is requested, then the
  tool returns a five-minute HTTPS URL without embedding or fetching
  attachment bytes.
- [x] Given a download-grant result, when it is returned, then it identifies the
  URL as a bearer secret for human use and excludes the raw token from logs.
- [x] Given a latest-submission read, when it succeeds, then the bounded summary
  may be returned but the description is absent.
- [x] Given a submission read by ID, when it succeeds, then its optional summary
  and description are returned without duplicated explanatory prose.

**Tasks**

- [x] Add request, submission, download-grant, control, and mapping read tools.
- [x] Map domain problems to stable MCP problem codes.
- [x] Add representative integration tests for every read family.

**Notes**

- The spec was revised on 2026-06-20 to defer source material and disclose
  submission context only through focused reads.
