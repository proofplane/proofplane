# 004 - Release Gate And Limitations

**Status:** Todo · **Depends on:** 001, 002, 003, reliability-observability/005 · **Spec:** [spec.md](../spec.md#release-gate)

**Summary** - Define and execute the final backend MVP validation record,
including explicit limitations and all required commands.

**Acceptance criteria**

- [ ] Given a clean checkout with documented prerequisites, when the release
  commands run, then formatting, Clippy, unit, integration, and end-to-end tests
  pass.
- [ ] Given the release documentation, when reviewed, then migrations,
  configuration effects, supported flows, and known limitations are explicit.
- [ ] Given any failing release command, when the ticket is evaluated, then it
  remains `Doing` or `Blocked`, never `Done`.

**Tasks**

- [ ] Add a release checklist and exact validation commands.
- [ ] Reconcile architecture, domain, API, and epic specs to shipped behavior.
- [ ] Record known limitations and operational assumptions.
- [ ] Run and record the complete release gate.
