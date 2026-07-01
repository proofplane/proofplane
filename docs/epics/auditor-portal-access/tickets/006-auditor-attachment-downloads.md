# 006 - Auditor Attachment Downloads

**Status:** Todo · **Depends on:** 005 · **Spec:** [spec.md](../spec.md#attachment-downloads)

**Summary** - Let verified auditor sessions download eligible evidence
attachments while keeping object storage private and rechecking eligibility on
every request.

**Acceptance criteria**

- [ ] Given a valid auditor session and uploaded unarchived attachment, when
  download is requested, then Proofplane streams it with safe download headers.
- [ ] Given pending, failed, malicious, archived, missing, or cross-workspace
  attachments, when requested, then download is rejected.
- [ ] Given grant or session revocation, when download is requested, then no
  bytes stream.
- [ ] Given logs and responses, when inspected, then object keys and attachment
  bytes are not logged or serialized as metadata.

**Tasks**

- [ ] Add auditor attachment download route.
- [ ] Recheck session, grant, workspace, attachment status, and object metadata.
- [ ] Reuse safe `Cache-Control`, `Referrer-Policy`, and `Content-Disposition`
  behavior.
- [ ] Emit identifier-only download audit logs.
- [ ] Add integration tests for success, rejection, revocation, and headers.
