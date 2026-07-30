# 002 - Machine Upload Grants

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#machine-upload-grant-persistence)

**Summary** - Add a one-file machine upload grant with preallocated submission
identity, declared metadata, short-lived authority, and agent-connection
provenance. This keeps machine semantics separate from browser grants.

**Acceptance criteria**

- [ ] Given authorized, valid preparation metadata, when a grant is issued, then
  its workspace, evidence, coverage, file declaration, submission ID, user,
  agent connection, and expiry are persisted.
- [ ] Given unavailable or cross-workspace evidence, when issuance is attempted,
  then it returns the concealed unavailable result and persists no grant.
- [ ] Given a tampered, expired, mismatched, or wrong-purpose credential, when
  it is verified, then it cannot authorize a transfer.
- [ ] Given existing human upload grants, when the machine schema ships, then
  their issuance and redemption semantics are unchanged.

**Tasks**

- [ ] Add typed machine-grant domain records and validation.
- [ ] Add the `agent_evidence_upload_grants` migration and repository methods.
- [ ] Add versioned credential issuance and verification.
- [ ] Enforce workspace, evidence, user, and agent-connection provenance.
- [ ] Add unit tests for claims and integration tests for persistence,
  expiration, concealment, and tenant isolation.
- [ ] Search modified runtime paths for `.expect(` and remove every occurrence.
