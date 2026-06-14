# 002 - Attachment Download Grants

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#download-grant-model)

**Summary** - Let an authorized actor issue a five-minute Proofplane URL that a
human can open to directly download one finalized attachment without a product
browser session.

**Acceptance criteria**

- [x] Given an uploaded attachment with matching stored metadata, when a grant is
  requested, then a signed URL scoped to that attachment is returned with a
  five-minute expiry.
- [x] Given a valid unexpired grant, when its URL is opened with GET, then bytes
  stream immediately with no-store/no-referrer headers.
- [x] Given a pending or finalizing attachment, when a grant is requested, then
  a stable `409 attachment_not_ready` response is returned and no grant exists.
- [x] Given a malicious, failed, missing, cross-submission, or cross-workspace
  attachment, when a grant is requested, then `404` is returned.
- [x] Given an expired or malformed grant, when it is opened, then `404` is
  returned and no byte stream begins.
- [x] Given repeated or concurrent GETs before expiry, when the grant remains
  valid, then each request may stream the attachment.
- [x] Given the attachment becomes ineligible or its metadata no longer matches,
  when an unexpired grant is opened, then download is rejected.
- [x] Given existing submission detail reads, when this ships, then attachment
  metadata remains visible without exposing object keys, tokens, or quarantined
  bytes.

**Tasks**

- [x] Add stateless download-grant JWT signing and verification.
- [x] Add workspace-scoped grant issuance with eligibility checks.
- [x] Add direct GET streaming with safe response headers.
- [x] Add expiry, repeat-GET, concurrency, and lifecycle-state tests.
- [x] Cover object-not-found, metadata-mismatch, and stream-failure behavior.
- [x] Verify JWTs and signing secrets never appear in structured application
  logs or persistence; document mandatory external query-parameter redaction.

**Notes**

- The spec was revised on 2026-06-14 to replace persisted opaque grants with
  stateless HS256 JWT URLs.
- The spec was revised on 2026-06-15 to validate portable filenames at upload
  and simplify the download `Content-Disposition` header.
