# 002 - Machine Upload Grants

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#machine-upload-grant-persistence)

**Summary** - Add a one-file machine upload grant with preallocated submission
identity, declared metadata, short-lived authority, and agent-connection
provenance. This keeps machine semantics separate from browser grants.

**Acceptance criteria**

- [x] Given authorized, valid preparation metadata, when a grant is issued, then
  its workspace, evidence, coverage, file declaration, submission ID, user,
  agent connection, and expiry are persisted.
- [x] Given unavailable or cross-workspace evidence, when issuance is attempted,
  then it returns the concealed unavailable result and persists no grant.
- [x] Given a tampered, expired, mismatched, or wrong-purpose credential, when
  it is verified, then it cannot authorize a transfer.
- [x] Given existing human upload grants, when the machine schema ships, then
  their issuance and redemption semantics are unchanged.

**Tasks**

- [x] Add typed machine-grant domain records and validation.
- [x] Add the `agent_evidence_upload_grants` migration and repository methods.
- [x] Add versioned credential issuance and verification.
- [x] Enforce workspace, evidence, user, and agent-connection provenance.
- [x] Add unit tests for claims and integration tests for persistence,
  expiration, concealment, and tenant isolation.
- [x] Search modified runtime paths for `.expect(` and remove every occurrence.

**Notes**

- The spec was revised to align evidence eligibility with the shipped active,
  paused, and retired status model.
