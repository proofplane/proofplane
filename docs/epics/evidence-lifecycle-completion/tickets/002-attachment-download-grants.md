# 002 - Attachment Download Grants

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#download-grant-model)

**Summary** - Let an authorized actor issue a five-minute Proofplane URL that a
human can open to directly download one finalized attachment without a product
browser session.

**Acceptance criteria**

- [ ] Given an uploaded attachment with matching stored metadata, when a grant is
  requested, then an opaque URL scoped to that attachment is returned with a
  five-minute expiry.
- [ ] Given a valid unexpired grant, when its URL is opened with GET, then bytes
  stream immediately with no-store/no-referrer headers.
- [ ] Given a pending or finalizing attachment, when a grant is requested, then
  a stable `409 attachment_not_ready` response is returned and no grant exists.
- [ ] Given a malicious, failed, missing, cross-submission, or cross-workspace
  attachment, when a grant is requested, then `404` is returned.
- [ ] Given an expired or malformed grant, when it is opened, then `404` is
  returned and no byte stream begins.
- [ ] Given repeated or concurrent GETs before expiry, when the grant remains
  valid, then each request may stream the attachment.
- [ ] Given the attachment becomes ineligible or its metadata no longer matches,
  when an unexpired grant is opened, then download is rejected.
- [ ] Given existing submission detail reads, when this ships, then attachment
  metadata remains visible without exposing object keys, tokens, or quarantined
  bytes.

**Tasks**

- [ ] Add the download-grant migration and token generation/hashing.
- [ ] Add workspace-scoped grant issuance with eligibility checks.
- [ ] Add direct GET streaming with safe response headers.
- [ ] Add expiry, repeat-GET, concurrency, and lifecycle-state tests.
- [ ] Cover object-not-found, metadata-mismatch, and stream-failure behavior.
- [ ] Verify raw tokens never appear in persistence, structured application
  logs, or analytics; document unavoidable infrastructure request-log exposure.
