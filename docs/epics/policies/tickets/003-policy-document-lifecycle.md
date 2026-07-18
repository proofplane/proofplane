# 003 - Policy Document Lifecycle

**Status:** Done · **Depends on:** [001](./001-policy-domain-and-persistence.md) · **Spec:** [spec.md](../spec.md#document-lifecycle)

**Summary** - Add one evidence-style document to each policy and safely reuse
the shared owned-document quarantine, malware-scan, and finalization pipeline.

**Acceptance criteria**

- [x] Given an active policy with no current document, when a valid file is
  accepted, then one document enters the existing lifecycle and can reach
  `uploaded` through scan and finalization.
- [x] Given any non-archived document, when another upload races or is
  attempted, then the second document is rejected without orphaning stored
  bytes.
- [x] Given an `uploaded`, `contains_virus`, or `failed` document, when it is
  archived, then it is hidden and a later upload is allowed; `pending` and
  `finalizing` documents reject archive.
- [x] Given stale, malformed, wrong-owner, metadata-mismatched, or retry-exhausted
  work, when a worker handles it, then policy state advances only according to
  the established safe retry/failure rules.
- [x] Given existing evidence document behavior, when typed-owner worker
  support ships, then all contracts use document terminology while lifecycle
  statuses, security rules, and outcomes remain unchanged.
- [x] Given an authenticated document creation, when the row is persisted and
  read, then `created_by_user_id` is derived from that transaction's user and
  returned as document metadata rather than accepted from the caller.

**Tasks**

- [x] Consolidate evidence and policy upload rows into typed, workspace-owned
  documents with a partial single-active policy index.
- [x] Add workspace-scoped document create, read, archive, and worker work
  repository operations.
- [x] Generalize scan/finalization work around an explicit typed owner without
  trusting object-key structure.
- [x] Add policy quarantine/final object-key namespaces and identifier-only
  audit events.
- [x] Reuse existing size, filename, integrity, object-store, scanner, and
  lifecycle rules.
- [x] Add unit and Docker-backed integration tests for migration, success, malicious and
  failed files, retries, concurrency, archival, and evidence regressions.
- [x] Persist, project, and test required document creator attribution.

**Notes**

- 2026-07-17: The spec now records the shared `documents` persistence model and
  application-enforced typed owner relationship.
- 2026-07-17: The shared `Document` domain type now owns all file metadata;
  evidence and policy keep document terminology only in boundary projections.
- 2026-07-17: The spec now records the corrected discriminated document
  identity used by worker and repository work records.
- 2026-07-17: Document terminology now applies consistently across domain,
  persistence, routes, worker events, MCP contracts, configuration, and docs.
- 2026-07-17: The spec now records required, context-derived document creator
  attribution and the delegated browser-session limitation.
