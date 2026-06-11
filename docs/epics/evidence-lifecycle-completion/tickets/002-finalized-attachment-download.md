# 002 - Finalized Attachment Download

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#retrieval-invariants)

**Summary** - Add a normal attachment-content endpoint that streams finalized
objects and makes every non-usable lifecycle state non-downloadable.

**Acceptance criteria**

- [ ] Given an uploaded attachment with matching stored metadata, when content
  is requested, then bytes stream with its content type and safe filename.
- [ ] Given a pending or finalizing attachment, when content is requested, then
  a stable `409 attachment_not_ready` response is returned.
- [ ] Given a malicious, failed, missing, cross-submission, or cross-workspace
  attachment, when content is requested, then `404` is returned.
- [ ] Given existing submission detail reads, when this ships, then attachment
  metadata remains visible without exposing quarantined bytes.

**Tasks**

- [ ] Add the workspace-scoped attachment download candidate query.
- [ ] Add service eligibility and object-metadata checks.
- [ ] Add the streaming route and stable error mapping.
- [ ] Add API/storage integration tests for every lifecycle state.
- [ ] Cover object-not-found and metadata-mismatch failures.
