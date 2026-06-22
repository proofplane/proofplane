# 003 - Evidence Freshness And Usability

**Status:** Todo · **Depends on:** evidence-lifecycle-completion/002 · **Spec:** [spec.md](../spec.md#evidence-freshness)

**Summary** - Derive packet-ready evidence freshness and usability from
Evidence Request rules, latest submissions, and finalized attachment state.

**Acceptance criteria**

- [ ] Given a request with a fresh latest submission and usable attachments,
  when readiness is evaluated, then its state is `current` with the evaluation
  timestamp.
- [ ] Given recently received evidence whose coverage end is outside the
  freshness window, when readiness is evaluated, then its state is `stale`.
- [ ] Given expired, missing, pending, malicious, or failed evidence, when
  readiness is evaluated, then `stale`, `missing`, or `unusable` is returned as
  specified.
- [ ] Given a fixed clock, when the same data is evaluated repeatedly, then the
  result is deterministic.
- [ ] Given existing submission and attachment responses, when this ships, then
  their contracts remain unchanged.

**Tasks**

- [ ] Add clock-injected freshness evaluator.
- [ ] Add repository read inputs for each request's latest evidence.
- [ ] Include derived state in the auditor-packet read model.
- [ ] Add focused unit tests and database-backed integration scenarios.

**Notes**

- The spec was revised on 2026-06-20 to remove the deferred source-material
  model from this evaluator.
- Freshness uses `coverage_end_at`, not `received_at`; this ticket serves packet
  readiness and is not required for the core MCP demo.
