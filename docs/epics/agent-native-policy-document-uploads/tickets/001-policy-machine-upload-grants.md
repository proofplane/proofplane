# 001 - Policy Machine Upload Grants

**Status:** Done · **Depends on:** agent-native-evidence-uploads/006 · **Spec:** [spec.md](../spec.md#machine-grant-persistence)

**Summary** - Add a one-file, policy-specific machine upload grant and reuse
the evidence flow's proven transfer primitives. This establishes agent
authority and declared metadata without broadening the human browser grant.

**Acceptance criteria**

- [x] Given an active policy with no current document and a valid declaration,
  when a grant is issued, then its tenant, policy, declaration, provenance, and
  expiry are durably recorded.
- [x] Given a missing, archived, or cross-workspace policy, when issuance is
  attempted, then it returns concealed unavailability and persists no grant.
- [x] Given a policy with a current document, when issuance is attempted, then
  a stable conflict is returned and no grant is persisted.
- [x] Given a tampered, expired, mismatched, or wrong-purpose credential, when
  it is verified, then it cannot authorize a transfer.
- [x] Given existing evidence and human policy grants, when this schema ships,
  then their issuance and redemption semantics are unchanged.

**Tasks**

- [x] Extract only reusable declared-file, credential, and transfer validation
  primitives from the evidence implementation.
- [x] Add the policy machine-grant aggregate and typed authority.
- [x] Add `agent_policy_document_upload_grants` persistence and migration.
- [x] Add versioned policy-purpose credential issuance and verification.
- [x] Enforce active-policy, empty-current-document, tenant, user, and agent
  connection eligibility.
- [x] Add unit and integration tests for validation, persistence, expiry,
  concealment, conflict, and tenant isolation.
- [x] Search modified runtime paths for `.expect(` and remove every occurrence.

**Notes**

- The policy machine grant remains distinct from both evidence machine grants
  and the existing browser-management grant.
- The spec revision records the shared 255-byte content-type bound applied to
  both machine-grant tables.
- The [persistence-boundary revision](../spec.md#revision-evidence-aggregate-and-snapshot-repository-alignment-2026-08-01)
  corrects the repository to persist complete snapshots while keeping policy
  eligibility, authorization, and provenance orchestration in services.
