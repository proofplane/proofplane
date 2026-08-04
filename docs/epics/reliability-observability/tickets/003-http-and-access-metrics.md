# 003 - HTTP And Access Metrics

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#metrics-contract)

**Summary** - Instrument HTTP, authentication, authorization, and readiness with
stable low-cardinality Prometheus metrics.

**Acceptance criteria**

- [ ] Given representative API traffic, when `/metrics` is read, then request
  count/duration/in-flight and access outcome metrics are present.
- [ ] Given parameterized routes, when metrics are inspected, then labels contain
  matched route patterns and never raw IDs or API keys.
- [ ] Given dependency readiness changes, when checks run, then the dependency
  status metric reflects the latest result.

**Tasks**

- [ ] Add HTTP middleware metrics using matched routes.
- [ ] Instrument authentication, authorization, and readiness outcomes.
- [ ] Document metric names, labels, and buckets.
- [ ] Add integration-v2 tests for presence and forbidden cardinality.
