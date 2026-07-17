# 006 - Auditor Submission Downloads

**Status:** Done · **Depends on:** 005 · **Spec:** [spec.md](../spec.md#submission-downloads)

**Summary** - Let verified auditor sessions download eligible evidence
submissions while keeping object storage private and rechecking eligibility on
every request.

**Acceptance criteria**

- [x] Given a valid auditor session and uploaded unarchived submission, when
  download is requested, then Proofplane streams it with safe download headers.
- [x] Given pending, failed, malicious, archived, missing, or cross-workspace
  submissions, when requested, then download is rejected.
- [x] Given grant or session revocation, when download is requested, then no
  bytes stream.
- [x] Given logs and responses, when inspected, then object keys and submission
  bytes are not logged or serialized as metadata.

**Tasks**

- [x] Add auditor submission download route.
- [x] Recheck session, grant, workspace, submission status, and object metadata.
- [x] Reuse safe `Cache-Control`, `Referrer-Policy`, and `Content-Disposition`
  behavior.
- [x] Emit identifier-only download audit logs.
- [x] Add integration tests for success, rejection, revocation, and headers.

**Notes**

- Spec revised to record the shipped direct auditor session download route.
