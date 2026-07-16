# 006 - Auditor Policy Read Model And Downloads

**Status:** Todo · **Depends on:** [001](./001-policy-domain-and-persistence.md), [003](./003-policy-attachment-lifecycle.md) · **Spec:** [spec.md](../spec.md#auditor-portal-read-model)

**Summary** - Extend the auditor backend with every active policy, per-control
policy relationships, safe attachment state, and session-bound downloads.

**Acceptance criteria**

- [ ] Given a valid auditor session, when portal data is loaded, then every
  active policy—including unattached and document-less policies—is returned in
  deterministic order and each control has its attached active policies.
- [ ] Given a non-uploaded current attachment, when policy data is returned,
  then its safe status is visible without a download action; archived policy
  and attachment rows are absent.
- [ ] Given an `uploaded` policy document and a valid auditor session, when its
  download route is requested, then Proofplane streams it with safe headers
  after rechecking all eligibility and object metadata.
- [ ] Given an invalid session or pending, failed, malicious, archived,
  missing, or cross-workspace state, when data or download is requested, then
  workspace data and bytes are not returned.
- [ ] Given existing auditor evidence reads and downloads, when policies ship,
  then their ordering, eligibility, routes, and behavior remain unchanged.

**Tasks**

- [ ] Extend auditor domain/repository/service response models with policies,
  mappings, and safe attachment metadata.
- [ ] Add deterministic ordering, active-row filtering, and download
  eligibility composition.
- [ ] Add the session-authenticated policy document download route and reuse
  safe object/header validation.
- [ ] Emit identifier-only policy read and download audit events.
- [ ] Add Docker-backed integration tests for complete catalogs, mapping
  composition, unavailable states, tenant/session isolation, downloads,
  revocation, metadata exclusion, and evidence regressions.
