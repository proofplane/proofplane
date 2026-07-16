# 003 - Policy Attachment Lifecycle

**Status:** Todo · **Depends on:** [001](./001-policy-domain-and-persistence.md) · **Spec:** [spec.md](../spec.md#attachment-lifecycle)

**Summary** - Add one evidence-style attachment to each policy and safely reuse
the quarantine, malware-scan, and finalization pipeline for a typed policy
attachment owner.

**Acceptance criteria**

- [ ] Given an active policy with no current attachment, when a valid file is
  accepted, then one attachment enters the existing lifecycle and can reach
  `uploaded` through scan and finalization.
- [ ] Given any non-archived attachment, when another upload races or is
  attempted, then the second attachment is rejected without orphaning stored
  bytes.
- [ ] Given an `uploaded`, `contains_virus`, or `failed` attachment, when it is
  archived, then it is hidden and a later upload is allowed; `pending` and
  `finalizing` attachments reject archive.
- [ ] Given stale, malformed, wrong-owner, metadata-mismatched, or retry-exhausted
  work, when a worker handles it, then policy state advances only according to
  the established safe retry/failure rules.
- [ ] Given existing evidence attachments, when typed-owner worker support
  ships, then their messages, object paths, statuses, and outcomes are
  unchanged.

**Tasks**

- [ ] Add policy attachment domain types on the schema and partial
  single-active index established by ticket 001.
- [ ] Add workspace-scoped attachment create, read, archive, and worker work
  repository operations.
- [ ] Generalize scan/finalization work around an explicit typed owner without
  trusting object-key structure.
- [ ] Add policy quarantine/final object-key namespaces and identifier-only
  audit events.
- [ ] Reuse existing size, filename, integrity, object-store, scanner, and
  lifecycle rules.
- [ ] Add unit and Docker-backed integration tests for success, malicious and
  failed files, retries, concurrency, archival, and evidence regressions.
