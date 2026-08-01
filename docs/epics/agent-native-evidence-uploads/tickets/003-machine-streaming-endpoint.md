# 003 - Machine Streaming Endpoint

**Status:** Done · **Depends on:** 001, 002 · **Spec:** [spec.md](../spec.md#streaming-endpoint)

**Summary** - Add the authenticated raw PUT endpoint that streams a declared
file into quarantine and creates a pending evidence submission through the
existing scan pipeline.

**Acceptance criteria**

- [x] Given a valid unexpired grant and matching headers and body, when the PUT
  completes, then it returns `201` with the preallocated submission ID,
  document ID, and `pending` status.
- [x] Given missing or invalid authority, when the endpoint is called, then one
  stable unavailable response conceals grant and tenant details.
- [x] Given an oversized body or mismatched content type, length, or checksum,
  when validation fails, then no submission or scan event commits and staged
  bytes are deleted.
- [x] Given a successful upload, when its submission is read, then user and
  agent-connection provenance and declared coverage are preserved.

**Tasks**

- [x] Register the PUT route with configured request limits.
- [x] Parse and verify the upload authorization scheme and declared headers.
- [x] Stream the body through transport-neutral quarantine ingestion.
- [x] Complete submission, document, and outbox creation with the grant.
- [x] Map validation and dependency failures to stable API responses.
- [x] Add HTTP integration tests for success, rejection, cleanup, provenance,
  and scanner handoff.
- [x] Search modified runtime paths for `.expect(` and remove every occurrence.

**Notes**

- The spec's 2026-07-31 revision records the machine-grant aggregate,
  full-snapshot two-operation repository, reusable workspace snapshot upsert
  helper, and dedicated upload workflow used by this ticket.
