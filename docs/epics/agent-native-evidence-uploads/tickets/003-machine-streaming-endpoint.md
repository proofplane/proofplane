# 003 - Machine Streaming Endpoint

**Status:** Todo · **Depends on:** 001, 002 · **Spec:** [spec.md](../spec.md#streaming-endpoint)

**Summary** - Add the authenticated raw PUT endpoint that streams a declared
file into quarantine and creates a pending evidence submission through the
existing scan pipeline.

**Acceptance criteria**

- [ ] Given a valid unexpired grant and matching headers and body, when the PUT
  completes, then it returns `201` with the preallocated submission ID,
  document ID, and `pending` status.
- [ ] Given missing or invalid authority, when the endpoint is called, then one
  stable unavailable response conceals grant and tenant details.
- [ ] Given an oversized body or mismatched content type, length, or checksum,
  when validation fails, then no submission or scan event commits and staged
  bytes are deleted.
- [ ] Given a successful upload, when its submission is read, then user and
  agent-connection provenance and declared coverage are preserved.

**Tasks**

- [ ] Register the PUT route with configured request limits.
- [ ] Parse and verify the upload authorization scheme and declared headers.
- [ ] Stream the body through transport-neutral quarantine ingestion.
- [ ] Complete submission, document, and outbox creation with the grant.
- [ ] Map validation and dependency failures to stable API responses.
- [ ] Add HTTP integration tests for success, rejection, cleanup, provenance,
  and scanner handoff.
- [ ] Search modified runtime paths for `.expect(` and remove every occurrence.
