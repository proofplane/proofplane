# 003 - Evidence Demo Seed

**Status:** Todo · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#demo-seed)

**Summary** - Extend local seed data with a realistic submission and finalized
attachment so the completed evidence flow is inspectable immediately.

**Acceptance criteria**

- [ ] Given a clean local state, when seed runs, then one sample submission and
  downloadable uploaded attachment exist for a seeded Evidence Request.
- [ ] Given seed has already run, when it runs again, then it does not create
  duplicate submissions, rows, or object files.
- [ ] Given a non-filesystem storage configuration, when seed runs, then it does
  not fabricate an uploaded attachment without matching object bytes.

**Tasks**

- [ ] Add deterministic submission and attachment identifiers.
- [ ] Write matching sample bytes and metadata through the filesystem adapter.
- [ ] Insert or reconcile the uploaded attachment row idempotently.
- [ ] Add seed integration coverage and update seed output.
