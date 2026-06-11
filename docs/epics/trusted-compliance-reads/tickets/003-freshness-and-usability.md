# 003 - Freshness And Usability

**Status:** Todo · **Depends on:** 001, evidence-lifecycle-completion/002 · **Spec:** [spec.md](../spec.md#freshness)

**Summary** - Derive source-material freshness and evidence usability from
Evidence Request rules, latest submissions, and finalized attachment state.

**Acceptance criteria**

- [ ] Given fresh uploaded evidence, when linked material is read, then its
  freshness is `current` with the evaluation timestamp.
- [ ] Given expired, missing, pending, malicious, or failed linked evidence,
  when material is read, then `stale` or `unusable` is returned as specified.
- [ ] Given a fixed clock, when the same data is evaluated repeatedly, then the
  result is deterministic.
- [ ] Given general control material without an Evidence Request link, when no
  submission exists, then it is not made stale solely by that absence.

**Tasks**

- [ ] Add clock-injected freshness evaluator.
- [ ] Add repository read inputs for latest linked evidence.
- [ ] Include derived state in get/search responses.
- [ ] Add focused unit tests and database-backed integration scenarios.
