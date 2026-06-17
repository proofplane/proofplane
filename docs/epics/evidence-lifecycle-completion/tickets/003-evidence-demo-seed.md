# 003 - Evidence Demo Seed

**Status:** Done · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#demo-seed)

**Summary** - Extend local seed data with a realistic submission and finalized
attachment so the completed evidence flow is inspectable immediately.

**Acceptance criteria**

- [x] Given a clean local state, when seed runs, then one sample submission and
  grant-eligible uploaded attachment exist for a seeded Evidence Request.
- [x] Given seed has already run, when it runs again, then it does not create
  duplicate submissions, rows, or object files.
- [x] Given a non-filesystem storage configuration, when seed runs, then it does
  not fabricate an uploaded attachment without matching object bytes.

**Tasks**

- [x] Add deterministic submission and attachment identifiers.
- [x] Write matching sample bytes and metadata through the filesystem adapter.
- [x] Insert or reconcile the uploaded attachment row idempotently.
- [x] Update seed output and verify through existing checks.

**Notes**

- Dedicated seed integration coverage was deferred because this is local/demo
  setup behavior; verification uses the existing attachment download checks and
  full repository check suite.
