# 006 - Auditor Attachment Downloads

**Status:** Done · **Depends on:** 005 · **Spec:** [spec.md](../spec.md#attachment-downloads)

**Summary** - Let verified auditor sessions download eligible evidence
attachments while keeping object storage private and rechecking eligibility on
every request.

**Acceptance criteria**

- [x] Given a valid auditor session and uploaded unarchived attachment, when
  download is requested, then Proofplane streams it with safe download headers.
- [x] Given pending, failed, malicious, archived, missing, or cross-workspace
  attachments, when requested, then download is rejected.
- [x] Given grant or session revocation, when download is requested, then no
  bytes stream.
- [x] Given logs and responses, when inspected, then object keys and attachment
  bytes are not logged or serialized as metadata.

**Tasks**

- [x] Add auditor attachment download route.
- [x] Recheck session, grant, workspace, attachment status, and object metadata.
- [x] Reuse safe `Cache-Control`, `Referrer-Policy`, and `Content-Disposition`
  behavior.
- [x] Emit identifier-only download audit logs.
- [x] Add integration tests for success, rejection, revocation, and headers.

**Notes**

- Spec revised to record the shipped direct auditor session download route.
